//! Small isolated player transforms, never the complete browser player program.
//! This is an independent implementation. The MIT rusty_ytdl reference was
//! consulted for response concepts; no upstream implementation is embedded.
use regex::Regex;
use rquickjs::{Context, Runtime};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use tuneka_core::*;
use url::Url;

type Cipher = Option<(String, String)>;
pub fn stream_url(format: &Value) -> Result<(Url, Cipher)> {
    if let Some(url) = format["url"].as_str() {
        return Ok((parse_stream_url(url)?, None));
    }
    let cipher = format["signatureCipher"]
        .as_str()
        .or_else(|| format["cipher"].as_str())
        .ok_or_else(|| Error::new(ErrorCode::Unsupported, "format has no downloadable URL"))?;
    if cipher.len() > 65536 {
        return Err(Error::new(ErrorCode::Protocol, "cipher exceeds size limit"));
    }
    let pairs: Vec<_> = url::form_urlencoded::parse(cipher.as_bytes()).collect();
    let url = pairs
        .iter()
        .find(|(k, _)| k == "url")
        .map(|(_, v)| v.as_ref())
        .ok_or_else(|| Error::new(ErrorCode::Protocol, "cipher lacks URL"))?;
    let signature = pairs
        .iter()
        .find(|(k, _)| k == "s")
        .map(|(_, v)| v.to_string());
    let parameter = pairs
        .iter()
        .find(|(k, _)| k == "sp")
        .map(|(_, v)| v.to_string())
        .unwrap_or_else(|| "signature".into());
    if !matches!(parameter.as_str(), "signature" | "sig" | "lsig") {
        return Err(Error::new(
            ErrorCode::Protocol,
            "unsupported signature parameter",
        ));
    }
    Ok((parse_stream_url(url)?, signature.map(|s| (parameter, s))))
}
fn parse_stream_url(value: &str) -> Result<Url> {
    if value.len() > 65536 {
        return Err(Error::new(
            ErrorCode::Protocol,
            "media URL exceeds size limit",
        ));
    }
    Url::parse(value).map_err(|_| Error::new(ErrorCode::Protocol, "invalid media URL"))
}

pub async fn transform(
    source: Arc<String>,
    input: String,
    n: bool,
    workers: Arc<Semaphore>,
) -> Result<String> {
    if input.is_empty() || input.len() > 4096 {
        return Err(Error::new(
            ErrorCode::Protocol,
            "invalid player transform input length",
        ));
    }
    let permit = workers
        .acquire_owned()
        .await
        .map_err(|_| Error::new(ErrorCode::Internal, "player workers are unavailable"))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let program = extract_program(&source, n)?;
        execute(&program, &input)
    })
    .await
    .map_err(|_| Error::new(ErrorCode::Internal, "player worker failed"))?
}

struct Program {
    name: String,
    source: String,
}
struct Function<'a> {
    name: &'a str,
    body: &'a str,
    source: String,
}

const MAX_DEFINITION_BYTES: usize = 128 * 1024;
const MAX_DEFINITION_TOTAL_BYTES: usize = 8 * 1024 * 1024;

fn functions(source: &str) -> Result<Vec<Function<'_>>> {
    let re=Regex::new(r"(?:^|[;,{\s])(?:var\s+|let\s+|const\s+)?([A-Za-z_$][\w$]*)\s*=\s*function\s*\([^)]*\)\s*\{|function\s+([A-Za-z_$][\w$]*)\s*\([^)]*\)\s*\{").expect("static regex");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut functions = Vec::new();
    let mut copied_bytes = 0usize;
    for capture in re.captures_iter(source) {
        if Instant::now() >= deadline || functions.len() >= 8192 {
            return Err(unsupported_indexed());
        }
        let full = capture.get(0).expect("function match");
        let name = capture
            .get(1)
            .or_else(|| capture.get(2))
            .expect("function name")
            .as_str();
        let start = full.end() - 1;
        let Some(end) = balanced_end(source, start) else {
            continue;
        };
        let Some(offset) = source[full.start()..start].find("function") else {
            continue;
        };
        let function_start = full.start() + offset;
        // Check before formatting/cloning. Browser-only outer functions may be
        // very large; skipping them leaves any needed oversized entry unsupported.
        let copy_bound = end - function_start + name.len() + 16;
        if copy_bound > MAX_DEFINITION_BYTES {
            continue;
        }
        if copy_bound > MAX_DEFINITION_TOTAL_BYTES - copied_bytes {
            return Err(unsupported_indexed());
        }
        let definition = if capture.get(1).is_some() {
            format!("var {name}={};", &source[function_start..end])
        } else {
            format!("{};", &source[function_start..end])
        };
        copied_bytes += definition.len();
        functions.push(Function {
            name,
            body: &source[start..end],
            source: definition,
        });
    }
    Ok(functions)
}

fn extract_program(source: &str, n: bool) -> Result<Program> {
    if source.len() > 10 * 1024 * 1024 {
        return Err(Error::new(
            ErrorCode::Protocol,
            "player script exceeds size limit",
        ));
    }
    let functions = functions(source)?;
    if let Some(program) = direct_indexed_program(source, &functions, n)? {
        return Ok(program);
    }
    if !n && let Some(program) = plasma_signature_program(source, &functions)? {
        return Ok(program);
    }
    if let Some(program) = indexed_program(source, &functions, n)? {
        return Ok(program);
    }
    let patterns: &[&str] = if n {
        &[
            r#"\.get\("n"\)\)\s*&&\s*\([^=]+=([A-Za-z_$][\w$]*)(?:\[\d+\])?\("#,
            r#"\.get\("n"\)\)\s*&&\s*\([^,]+,\s*[^=]+=([A-Za-z_$][\w$]*)\("#,
        ]
    } else {
        &[
            r#"\.sig\|\|([A-Za-z_$][\w$]*)\("#,
            r#"["']signature["']\s*,\s*([A-Za-z_$][\w$]*)\("#,
            r#"\bc&&\(c=([A-Za-z_$][\w$]*)\(decodeURIComponent"#,
        ]
    };
    let called = patterns.iter().find_map(|pattern| {
        Regex::new(pattern)
            .ok()?
            .captures(source)?
            .get(1)
            .map(|m| m.as_str())
    });
    let selected = if let Some(name) = called {
        let matches = functions
            .iter()
            .filter(|f| f.name == name)
            .collect::<Vec<_>>();
        (matches.len() == 1).then(|| matches[0])
    } else {
        let candidates = functions
            .iter()
            .filter(|f| {
                if n {
                    f.body.contains("enhanced_except_")
                } else {
                    (f.body.contains(".split(\"\")") || f.body.contains(".split('')"))
                        && (f.body.contains(".join(\"\")") || f.body.contains(".join('')"))
                        && !f.body.contains("enhanced_except_")
                }
            })
            .collect::<Vec<_>>();
        (candidates.len() == 1).then(|| candidates[0])
    }
    .ok_or_else(|| {
        Error::new(
            ErrorCode::Unsupported,
            if n {
                "player n-transform layout is unsupported or ambiguous"
            } else {
                "player signature layout is unsupported or ambiguous"
            },
        )
    })?;
    let mut source_code = selected.source.clone();
    let mut pending = vec![selected.body.to_owned()];
    let mut seen = HashSet::from([selected.name.to_owned()]);
    let reference =
        Regex::new(r"(?:^|[^A-Za-z0-9_$])([A-Za-z_$][\w$]*)\s*(?:\.[A-Za-z_$][\w$]*\s*\(|\[|\()")
            .expect("static regex");
    while let Some(body) = pending.pop() {
        for capture in reference.captures_iter(&body) {
            let name = capture.get(1).expect("capture").as_str();
            if !seen.insert(name.into()) {
                continue;
            }
            if seen.len() > 64 {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "player transform dependency limit exceeded",
                ));
            }
            if let Some(function) = functions.iter().find(|f| f.name == name) {
                source_code.insert_str(0, &function.source);
                pending.push(function.body.into());
                continue;
            }
            let pattern = format!(
                r"(?:var|let|const)\s+{}\s*=\s*([\[{{])",
                regex::escape(name)
            );
            if let Some(capture) = Regex::new(&pattern)
                .expect("escaped regex")
                .captures(source)
            {
                let start = capture.get(1).expect("capture").start();
                if let Some(end) = balanced_end(source, start) {
                    let body = &source[start..end];
                    if body.len() > 65536 {
                        return Err(Error::new(
                            ErrorCode::Unsupported,
                            "player helper exceeds size limit",
                        ));
                    }
                    source_code.insert_str(0, &format!("var {name}={body};"));
                    pending.push(body.into());
                }
            }
        }
        if source_code.len() > 131072 {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "player transform exceeds size limit",
            ));
        }
    }
    Ok(Program {
        name: selected.name.into(),
        source: source_code,
    })
}

// Revision 21090930 places the signature challenge in a direct table-indexed
// split/helper/join branch. Select it through the signature URL call site;
// the n challenge has a separate dispatcher.
fn direct_indexed_program(
    source: &str,
    functions: &[Function<'_>],
    n: bool,
) -> Result<Option<Program>> {
    if n {
        return Ok(None);
    }
    let call_re = Regex::new(
        r"if\([A-Za-z_$][\w$]*\.s\)\{[^{}]{0,128}\.sp;let\s+[A-Za-z_$][\w$]*=([A-Za-z_$][\w$]*)\((\d+),(\d+),([A-Za-z_$][\w$]*)\((\d+),(\d+),[A-Za-z_$][\w$]*\.s\)\);",
    )
    .expect("static regex");
    let calls = call_re.captures_iter(source).collect::<Vec<_>>();
    if calls.is_empty() {
        return Ok(None);
    }
    if calls.len() != 1 {
        return Err(unsupported_indexed());
    }
    let call = &calls[0];
    let parse = |index: usize| {
        call[index]
            .parse::<u32>()
            .map_err(|_| unsupported_indexed())
    };
    let (first, second, decode_first, _decode_second) =
        (parse(2)?, parse(3)?, parse(5)?, parse(6)?);
    let key = first ^ second;
    let unique_function = |name: &str| -> Result<&Function<'_>> {
        let matches = functions
            .iter()
            .filter(|function| function.name == name)
            .collect::<Vec<_>>();
        (matches.len() == 1)
            .then(|| matches[0])
            .ok_or_else(unsupported_indexed)
    };
    let selected = unique_function(&call[1])?;
    if !selected.body.contains("(q|16)==q") {
        return Ok(None);
    }
    let decoder = unique_function(&call[4])?;
    if !decoder.body.contains("q-1&7||(J=decodeURIComponent(W))")
        || (decode_first.wrapping_sub(1) & 7) != 0
        || !selected.body.contains("(q|16)==q")
        || (first | 16) != first
    {
        return Err(unsupported_indexed());
    }
    let split_re = Regex::new(r"W\[([A-Za-z_$][\w$]*)\[(\d+)\]\]\(([A-Za-z_$][\w$]*)\[(\d+)\]\)")
        .expect("static regex");
    let join_re = Regex::new(
        r"[A-Za-z_$][\w$]*\[([A-Za-z_$][\w$]*)\[[A-Za-z_$][\w$]*\^(\d+)\]\]\(([A-Za-z_$][\w$]*)\[(\d+)\]\)",
    )
    .expect("static regex");
    let split = split_re
        .captures(selected.body)
        .ok_or_else(unsupported_indexed)?;
    let join = join_re
        .captures(selected.body)
        .ok_or_else(unsupported_indexed)?;
    if split[1] != split[3] || split[1] != join[1] || join[1] != join[3] {
        return Err(unsupported_indexed());
    }
    let table_name = &split[1];
    let table_re = Regex::new(
        r#"(?:^|[;,\s])([A-Za-z_$][\w$]*)\s*=\s*("(?:\\.|[^"\\])*")\.split\(("(?:\\.|[^"\\])*")\)"#,
    )
    .expect("static regex");
    let tables = table_re
        .captures_iter(source)
        .filter(|capture| &capture[1] == table_name)
        .collect::<Vec<_>>();
    if tables.len() != 1 || tables[0][2].len() > 65536 {
        return Err(unsupported_indexed());
    }
    let value = serde_json::from_str::<String>(&tables[0][2]).map_err(|_| unsupported_indexed())?;
    let separator =
        serde_json::from_str::<String>(&tables[0][3]).map_err(|_| unsupported_indexed())?;
    if separator.is_empty() {
        return Err(unsupported_indexed());
    }
    let strings = value.split(&separator).take(257).collect::<Vec<_>>();
    if !(10..=256).contains(&strings.len()) {
        return Err(unsupported_indexed());
    }
    let split_index = split[2]
        .parse::<usize>()
        .map_err(|_| unsupported_indexed())?;
    let split_separator = split[4]
        .parse::<usize>()
        .map_err(|_| unsupported_indexed())?;
    let join_index = join[2].parse::<u32>().map_err(|_| unsupported_indexed())? ^ key;
    let join_separator = join[4]
        .parse::<usize>()
        .map_err(|_| unsupported_indexed())?;
    if strings.get(split_index) != Some(&"split")
        || strings.get(join_index as usize) != Some(&"join")
        || strings.get(split_separator) != Some(&"")
        || strings.get(join_separator) != Some(&"")
    {
        return Err(unsupported_indexed());
    }
    let helper_re = Regex::new(&format!(
        r"([A-Za-z_$][\w$]*)\[{}\[[A-Za-z_$][\w$]*\^(\d+)\]\]\([A-Za-z_$][\w$]*,",
        regex::escape(table_name)
    ))
    .expect("escaped regex");
    let helper = helper_re
        .captures(selected.body)
        .ok_or_else(unsupported_indexed)?;
    let helper_name = &helper[1];
    let helper_assignment = Regex::new(&format!(
        r"(?:^|[;,\s]){}\s*=\s*(\{{)",
        regex::escape(helper_name)
    ))
    .expect("escaped regex");
    let assignments = helper_assignment.captures_iter(source).collect::<Vec<_>>();
    if assignments.len() != 1 {
        return Err(unsupported_indexed());
    }
    let start = assignments[0].get(1).expect("helper body").start();
    let end = balanced_end(source, start).ok_or_else(unsupported_indexed)?;
    let helper_object = &source[start..end];
    if helper_object.len() > 65536 || !helper_object.contains("function") {
        return Err(unsupported_indexed());
    }
    let code = format!(
        "var {table_name}={};var {helper_name}={helper_object};{}function __tuneka_direct_indexed(input){{var result={}({first},{second},decodeURIComponent(input));if(typeof result!==\"string\")throw Error('player transform result type');return result;}}",
        serde_json::to_string(&strings)?,
        selected.source,
        selected.name
    );
    if code.len() > 131072 {
        return Err(unsupported_indexed());
    }
    Ok(Some(Program {
        name: "__tuneka_direct_indexed".into(),
        source: code,
    }))
}

// The current mobile plasma player performs signature operations in a direct
// split/helper/join branch. Its indexed dispatcher is unrelated to the
// signature and must not be used to guess a selector for that operation.
fn plasma_signature_program(source: &str, functions: &[Function<'_>]) -> Result<Option<Program>> {
    let call_re = Regex::new(
        r"if\([A-Za-z_$][\w$]*\.s\)\{[^{}]{0,128}\.sp;let\s+[A-Za-z_$][\w$]*=([A-Za-z_$][\w$]*)\((\d+),(\d+),[A-Za-z_$][\w$]*\(\d+,\d+,[A-Za-z_$][\w$]*\.s\)\);",
    )
    .expect("static regex");
    let calls = call_re.captures_iter(source).collect::<Vec<_>>();
    if calls.is_empty() {
        return Ok(None);
    }
    if calls.len() != 1 {
        return Err(unsupported_indexed());
    }
    let call = &calls[0];
    let first = call[2].parse::<u32>().map_err(|_| unsupported_indexed())?;
    let second = call[3].parse::<u32>().map_err(|_| unsupported_indexed())?;
    let key = first ^ second;
    let selected = functions
        .iter()
        .filter(|function| function.name == &call[1])
        .collect::<Vec<_>>();
    if selected.len() != 1 {
        return Err(unsupported_indexed());
    }
    let selected = selected[0];
    let split_re =
        Regex::new(r"W\[([A-Za-z_$][\w$]*)\[Z\^(\d+)\]\]\(([A-Za-z_$][\w$]*)\[Z\^(\d+)\]\)")
            .expect("static regex");
    let join_re =
        Regex::new(r"G\[([A-Za-z_$][\w$]*)\[Z\^(\d+)\]\]\(([A-Za-z_$][\w$]*)\[Z\^(\d+)\]\)")
            .expect("static regex");
    let Some(split) = split_re.captures(selected.body) else {
        return Err(unsupported_indexed());
    };
    let Some(join) = join_re.captures(selected.body) else {
        return Err(unsupported_indexed());
    };
    let guard_re = Regex::new(r"if\(\(r\+(\d+)&(\d+)\)==(\d+)\)\{").expect("static regex");
    let selected_branches = guard_re
        .captures_iter(selected.body)
        .filter_map(|guard| {
            let brace = guard.get(0)?.end() - 1;
            let end = balanced_end(selected.body, brace)?;
            let contains_transform = brace < split.get(0)?.start() && join.get(0)?.end() <= end;
            let (offset, mask, expected) = (
                guard[1].parse::<u32>().ok()?,
                guard[2].parse::<u32>().ok()?,
                guard[3].parse::<u32>().ok()?,
            );
            contains_transform.then_some((offset, mask, expected))
        })
        .collect::<Vec<_>>();
    if selected_branches.len() != 1
        || (first.wrapping_add(selected_branches[0].0) & selected_branches[0].1)
            != selected_branches[0].2
    {
        return Err(unsupported_indexed());
    }
    if split[1] != split[3] || split[1] != join[1] || join[1] != join[3] {
        return Err(unsupported_indexed());
    }
    let table_name = &split[1];
    let table_re = Regex::new(
        r#"(?:^|[;,\s])([A-Za-z_$][\w$]*)\s*=\s*("(?:\\.|[^"\\])*")\.split\(("(?:\\.|[^"\\])*")\)"#,
    )
    .expect("static regex");
    let tables = table_re
        .captures_iter(source)
        .filter(|capture| &capture[1] == table_name)
        .collect::<Vec<_>>();
    if tables.len() != 1 || tables[0][2].len() > 65536 {
        return Err(unsupported_indexed());
    }
    let value = serde_json::from_str::<String>(&tables[0][2]).map_err(|_| unsupported_indexed())?;
    let separator =
        serde_json::from_str::<String>(&tables[0][3]).map_err(|_| unsupported_indexed())?;
    if separator.is_empty() {
        return Err(unsupported_indexed());
    }
    let strings = value.split(&separator).take(257).collect::<Vec<_>>();
    if !(10..=256).contains(&strings.len()) {
        return Err(unsupported_indexed());
    }
    for (capture, expected) in [(&split, "split"), (&join, "join")] {
        let index = capture[2]
            .parse::<u32>()
            .map_err(|_| unsupported_indexed())?
            ^ key;
        let separator_index = capture[4]
            .parse::<u32>()
            .map_err(|_| unsupported_indexed())?
            ^ key;
        if strings.get(index as usize) != Some(&expected)
            || strings.get(separator_index as usize) != Some(&"")
        {
            return Err(unsupported_indexed());
        }
    }
    let helper_re = Regex::new(&format!(
        r"([A-Za-z_$][\w$]*)\[{}\[Z\^(?:\d+|4E3)\]\]\(G,",
        regex::escape(table_name)
    ))
    .expect("escaped regex");
    let Some(helper) = helper_re.captures(selected.body) else {
        return Err(unsupported_indexed());
    };
    let helper_name = &helper[1];
    let helper_assignment = Regex::new(&format!(
        r"(?:^|[;,\s]){}\s*=\s*(\{{)",
        regex::escape(helper_name)
    ))
    .expect("escaped regex");
    let assignments = helper_assignment.captures_iter(source).collect::<Vec<_>>();
    if assignments.len() != 1 {
        return Err(unsupported_indexed());
    }
    let start = assignments[0].get(1).expect("helper body").start();
    let end = balanced_end(source, start).ok_or_else(unsupported_indexed)?;
    let helper_object = &source[start..end];
    if helper_object.len() > 65536 || !helper_object.contains("function") {
        return Err(unsupported_indexed());
    }
    let code = format!(
        "var {table_name}={};var {helper_name}={helper_object};{}function __tuneka_plasma_signature(input){{var result={}({first},{second},decodeURIComponent(input));if(typeof result!==\"string\")throw Error('player transform result type');return result;}}",
        serde_json::to_string(&strings)?,
        selected.source,
        selected.name
    );
    if code.len() > 131072 {
        return Err(unsupported_indexed());
    }
    Ok(Some(Program {
        name: "__tuneka_plasma_signature".into(),
        source: code,
    }))
}

// Current players share arithmetic-dispatched functions between transforms and
// unrelated browser operations. Select only a split/join branch tied to the
// indexed string table, then keep function definitions (never browser startup).
fn indexed_program(source: &str, functions: &[Function<'_>], n: bool) -> Result<Option<Program>> {
    let table_re = Regex::new(
        r#"(?:^|[;,\s])([A-Za-z_$][\w$]*)\s*=\s*("(?:\\.|[^"\\])*")\.split\(("(?:\\.|[^"\\])*")\)"#,
    )
    .expect("static regex");
    let mut table = None;
    for capture in table_re.captures_iter(source) {
        if capture[2].len() > 65536 {
            continue;
        }
        let Ok(value) = serde_json::from_str::<String>(&capture[2]) else {
            continue;
        };
        let Ok(separator) = serde_json::from_str::<String>(&capture[3]) else {
            continue;
        };
        if separator.is_empty() {
            continue;
        }
        let strings: Vec<String> = value
            .split(&separator)
            .take(257)
            .map(str::to_owned)
            .collect();
        if (10..=256).contains(&strings.len())
            && ["split", "join", "signatureCipher", "n"]
                .iter()
                .all(|value| strings.iter().any(|s| s == value))
        {
            if table.is_some() {
                return Err(unsupported_indexed());
            }
            table = Some((capture[1].to_owned(), strings));
        }
    }
    let Some((table_name, strings)) = table else {
        return Ok(None);
    };
    let plasma_table = n
        && table_name == "a"
        && strings.first().map(String::as_str) == Some("1")
        && [(5, "set"), (62, "n"), (13, "")]
            .iter()
            .all(|(index, value)| strings.get(*index).map(String::as_str) == Some(*value));
    if !plasma_table && strings.first().is_none_or(|s| !s.is_empty()) {
        return Err(unsupported_indexed());
    }
    let split_index = strings
        .iter()
        .position(|s| s == "split")
        .expect("checked table") as u32;
    let join_index = strings
        .iter()
        .position(|s| s == "join")
        .expect("checked table") as u32;
    let mut candidates = Vec::new();
    let indexed_access = format!("[{table_name}[");
    let discovery_deadline = Instant::now() + Duration::from_secs(5);
    for function in functions {
        if Instant::now() >= discovery_deadline {
            return Err(unsupported_indexed());
        }
        if !function.body.contains(&indexed_access) || (n && !function.body.contains("typeof ")) {
            continue;
        }
        let Some(parameters) = function
            .source
            .split("function")
            .nth(1)
            .and_then(|s| s.split_once('('))
            .and_then(|(_, s)| s.split_once(')'))
            .map(|(s, _)| s.split(',').map(str::trim).collect::<Vec<_>>())
        else {
            continue;
        };
        if parameters.len() < 3 {
            continue;
        }
        let split_re = Regex::new(&format!(
            r"\b([A-Za-z_$][\w$]*)\s*=\s*{}\[{}\[([A-Za-z_$][\w$]*)\^(\d+)\]\]\({}\[(?:([A-Za-z_$][\w$]*)\^)?(\d+)\]\)",
            regex::escape(parameters[2]),
            regex::escape(&table_name),
            regex::escape(&table_name)
        ))
        .expect("escaped regex");
        for capture in split_re.captures_iter(function.body) {
            if Instant::now() >= discovery_deadline {
                return Err(unsupported_indexed());
            }
            let Ok(split_xor) = capture[3].parse::<u32>() else {
                continue;
            };
            let key = split_xor ^ split_index;
            if capture
                .get(4)
                .is_some_and(|var| var.as_str() != &capture[2])
            {
                continue;
            }
            let Ok(separator_number) = capture[5].parse::<u32>() else {
                continue;
            };
            let separator_index = if capture.get(4).is_some() {
                separator_number ^ key
            } else {
                separator_number
            };
            if strings.get(separator_index as usize).map(String::as_str) != Some("") {
                continue;
            }
            let separator_expression = capture.get(4).map_or_else(
                || capture[5].to_owned(),
                |var| format!("{}^{}", var.as_str(), &capture[5]),
            );
            let join = format!(
                "{}[{}[{}^{}]]({}[{}])",
                &capture[1],
                table_name,
                &capture[2],
                key ^ join_index,
                table_name,
                separator_expression
            );
            if !function.body.contains(&join) {
                continue;
            }
            let guard_re = Regex::new(&format!(
                r"typeof\s+([A-Za-z_$][\w$]*)\s*===\s*{}\[{}\^(\d+)\]",
                regex::escape(&table_name),
                regex::escape(&capture[2])
            ))
            .expect("escaped regex");
            let guard = guard_re.captures(function.body);
            if n != guard.is_some() {
                continue;
            }
            let selector = if n {
                let mask_re = Regex::new(&format!(
                    r"\(\({}\|(\d+)\)=={}\)",
                    regex::escape(parameters[0]),
                    regex::escape(parameters[0])
                ))
                .expect("escaped regex");
                let Some(mask) = mask_re
                    .captures(function.body)
                    .and_then(|c| c[1].parse::<u32>().ok())
                else {
                    continue;
                };
                (mask, key ^ mask)
            } else {
                // A shared dispatcher's other branches can have the same XOR
                // key. Tie signature selection to the predicate guarding its
                // split/join block, rather than the first numeric call alone.
                let branch_re = Regex::new(&format!(
                    r"if\(\({}\+(\d+)&(\d+)\)==(\d+)\)\{{\s*(?:var\s+)?{}\s*=",
                    regex::escape(parameters[0]),
                    regex::escape(&capture[1])
                ))
                .expect("escaped regex");
                let Some(branch) = branch_re.captures(function.body) else {
                    continue;
                };
                let Some((offset, mask, expected)) = branch[1]
                    .parse::<u32>()
                    .ok()
                    .zip(branch[2].parse::<u32>().ok())
                    .zip(branch[3].parse::<u32>().ok())
                    .map(|((a, b), c)| (a, b, c))
                else {
                    continue;
                };
                let call_re = Regex::new(&format!(
                    r"(?:^|[^A-Za-z0-9_$]){}\((\d+),(\d+),",
                    regex::escape(function.name)
                ))
                .expect("escaped regex");
                let Some(selector) = call_re
                    .captures_iter(source)
                    .filter_map(|c| Some((c[1].parse::<u32>().ok()?, c[2].parse::<u32>().ok()?)))
                    .find(|(a, b)| a ^ b == key && (a.wrapping_add(offset) & mask) == expected)
                else {
                    continue;
                };
                selector
            };
            if plasma_table {
                let wrappers = functions
                    .iter()
                    .filter(|candidate| candidate.name == "My")
                    .collect::<Vec<_>>();
                let wrapper_key = 2686 ^ 62;
                if function.name != "xr"
                    || !function.source.contains("function(r,A,W,h){var J=A^r;")
                    || wrappers.len() != 1
                    || !wrappers[0].source.contains("function(r,A,W,h){var J=A^r;")
                    || !wrappers[0].body.contains(
                        "if(r>>2<8&&(r|7)>>3>=2){var Z=xr(J^2664,J^1983,W);h[a[5]](a[J^2686],Z)",
                    )
                    || selector != (wrapper_key ^ 2664, wrapper_key ^ 1983)
                    || strings.get((key ^ 3551) as usize).map(String::as_str) != Some("split")
                    || strings.get((key ^ 3481) as usize).map(String::as_str) != Some("join")
                {
                    continue;
                }
            }
            let guard_constant = if let Some(guard) = guard {
                let guard_index = guard[2].parse::<u32>().map_err(|_| unsupported_indexed())? ^ key;
                if strings.get(guard_index as usize).map(String::as_str) != Some("undefined") {
                    continue;
                }
                let constant_re = Regex::new(&format!(
                    r"(?:var|let|const)\s+{}\s*=\s*(-?\d+)\s*[;,]",
                    regex::escape(&guard[1])
                ))
                .expect("escaped regex");
                let constants = constant_re.captures_iter(source).collect::<Vec<_>>();
                if constants.len() != 1 {
                    return Err(unsupported_indexed());
                }
                format!("var {}={};", &guard[1], &constants[0][1])
            } else {
                String::new()
            };
            candidates.push((function, selector, guard_constant));
        }
    }
    if n && table_name == "I"
        && [
            (1, "split"),
            (36, "join"),
            (50, "undefined"),
            (10, "set"),
            (18, "n"),
        ]
        .iter()
        .all(|(index, value)| strings.get(*index).map(String::as_str) == Some(*value))
    {
        // The 21090930 player invokes its n branch through U_, rather than
        // calling the shared dispatcher with literal arguments. Derive both
        // selector arguments from that URL set("n", ...) call site.
        let direct = functions
            .iter()
            .filter(|function| {
                function.name == "of"
                    && function.source.contains("function(q,K,W,O){var u=K^q,J;")
                    && function.body.contains("if((q&121)==q)a:{l=W[I[1]](I[0]);")
                    && function.body.contains("typeof RD$===I[u^2086]")
                    && function.body.contains("J=l[I[u^2096]](I[0])")
                    && function.body.contains("J=I[u^2125]+")
            })
            .collect::<Vec<_>>();
        let wrappers = functions
            .iter()
            .filter(|function| function.name == "U_")
            .collect::<Vec<_>>();
        let wrapper_matches = wrappers.len() == 1
            && wrappers[0]
                .source
                .contains("function(q,K,W,O,u){var J=K^q;")
            && wrappers[0]
                .body
                .contains("if(((q^60)&7)==1){var l=of(J^1340,J^3368,W);O[I[J^1343]](I[J^1319],l)");
        let constant = Regex::new(r"(?:var|let|const)\s+RD\$\s*=\s*(-?\d+)\s*[;,]")
            .expect("static regex")
            .captures_iter(source)
            .collect::<Vec<_>>();
        if direct.len() == 1 && wrapper_matches && constant.len() == 1 {
            let key = 2086 ^ 50;
            let wrapper_key = 1343 ^ 10;
            let first = wrapper_key ^ 1340;
            let second = wrapper_key ^ 3368;
            if key == (2096 ^ 36)
                && wrapper_key == (1319 ^ 18)
                && first ^ second == key
                && (first & 121) == first
            {
                candidates.push((
                    direct[0],
                    (first, second),
                    format!("var RD$={};", &constant[0][1]),
                ));
            }
        }
    }
    if candidates.len() != 1 {
        return Err(unsupported_indexed());
    }
    let (selected, (first, second), guard) = candidates.pop().expect("one candidate");
    let mut code = format!(
        "var {table_name}={};{guard}",
        serde_json::to_string(&strings)?
    );
    let mut pending = vec![selected.name.to_owned()];
    let mut seen = HashSet::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    let object_re =
        Regex::new(r"(?:var|let|const)\s+([A-Za-z_$][\w$]*)\s*=\s*(\{)").expect("static regex");
    let mut objects: HashMap<&str, Vec<usize>> = HashMap::new();
    for capture in object_re.captures_iter(source) {
        objects
            .entry(capture.get(1).expect("object name").as_str())
            .or_default()
            .push(capture.get(2).expect("object body").start());
    }

    let identifier = Regex::new(r"[A-Za-z_$][\w$]*").expect("static regex");
    let assigned =
        Regex::new(r"(?:^|[^A-Za-z0-9_$.])([A-Za-z_$][\w$]*)\s*=").expect("static regex");
    let parameters =
        Regex::new(r"function\s*\(([^)]*)\)|catch\s*\(([^)]*)\)").expect("static regex");
    let declared = Regex::new(r"(?:var|let|const)\s+([A-Za-z_$][\w$]*)").expect("static regex");
    let declaration = Regex::new(r"(?:var|let|const)\s+([^;]+)").expect("static regex");
    let binding = Regex::new(r"^\s*([A-Za-z_$][\w$]*)\s*(?:=|$)").expect("static regex");
    let arrows = Regex::new(r"(?:\(([^)]*)\)|([A-Za-z_$][\w$]*))\s*=>").expect("static regex");
    while let Some(name) = pending.pop() {
        if Instant::now() >= deadline {
            return Err(unsupported_indexed());
        }
        if !seen.insert(name.clone()) {
            continue;
        }
        if seen.len() > 128 {
            return Err(unsupported_indexed());
        }
        let matches = functions
            .iter()
            .filter(|function| function.name == name)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(unsupported_indexed());
        }
        let function = matches[0];
        code.push_str(&function.source);
        if code.len() > 131072 {
            return Err(unsupported_indexed());
        }
        let mut locals = assigned
            .captures_iter(&function.source)
            .filter(|capture| {
                function
                    .source
                    .as_bytes()
                    .get(capture.get(0).expect("assignment capture").end())
                    != Some(&b'=')
            })
            .map(|c| c[1].to_owned())
            .collect::<HashSet<_>>();
        locals.extend(
            declared
                .captures_iter(&function.source)
                .map(|c| c[1].to_owned()),
        );
        for capture in declaration.captures_iter(&function.source) {
            for part in capture[1].split(',') {
                if let Some(binding) = binding.captures(part) {
                    locals.insert(binding[1].to_owned());
                }
            }
        }
        for capture in arrows.captures_iter(&function.source) {
            let params = capture
                .get(1)
                .or_else(|| capture.get(2))
                .expect("arrow parameters");
            locals.extend(
                identifier
                    .find_iter(params.as_str())
                    .map(|m| m.as_str().to_owned()),
            );
        }
        for capture in parameters.captures_iter(&function.source) {
            let parameters = capture
                .get(1)
                .or_else(|| capture.get(2))
                .expect("parameter capture");
            locals.extend(
                identifier
                    .find_iter(parameters.as_str())
                    .map(|m| m.as_str().to_owned()),
            );
        }
        for (index, token) in identifier.find_iter(function.body).enumerate() {
            if index % 128 == 0 && Instant::now() >= deadline {
                return Err(unsupported_indexed());
            }
            let name = token.as_str();
            if locals.contains(name)
                || function.body[..token.start()].trim_end().ends_with('.')
                || function.body[token.end()..].trim_start().starts_with(':')
            {
                continue;
            }
            if functions.iter().any(|f| f.name == name) {
                pending.push(name.to_owned());
            } else if name != table_name
                && !seen.contains(name)
                && let Some(starts) = objects.get(name)
            {
                if starts.len() != 1 {
                    return Err(unsupported_indexed());
                }
                let start = starts[0];
                let end = balanced_end(source, start).ok_or_else(unsupported_indexed)?;
                let object = &source[start..end];
                // Pure helper maps are permitted; initializer execution still
                // shares the same QuickJS memory/time/stack quotas.
                if object.len() > 65536 || !object.contains("function") {
                    return Err(unsupported_indexed());
                }
                code.push_str(&format!("var {name}={object};"));
                if code.len() > 131072 || seen.len() >= 128 {
                    return Err(unsupported_indexed());
                }
                seen.insert(name.to_owned());
            }
        }
        if code.len() > 131072 {
            return Err(unsupported_indexed());
        }
    }
    let argument = if n {
        "input"
    } else {
        "decodeURIComponent(input)"
    };
    let error_check = if n {
        let error_re=Regex::new(&format!(r"catch\s*\([^)]*\)\s*\{{\s*[A-Za-z_$][\w$]*\s*=\s*{}\[[A-Za-z_$][\w$]*\^(\d+)\]\s*\+\s*[A-Za-z_$][\w$]*\s*;",regex::escape(&table_name))).expect("escaped regex");
        let markers = error_re
            .captures_iter(selected.body)
            .filter_map(|capture| {
                capture[1]
                    .parse::<u32>()
                    .ok()
                    .and_then(|index| strings.get((first ^ second ^ index) as usize))
            })
            .collect::<Vec<_>>();
        if markers.len() != 1 || markers[0].is_empty() {
            return Err(unsupported_indexed());
        }
        format!(
            "if(result.startsWith({}))throw Error('player transform failure marker');",
            serde_json::to_string(markers[0])?
        )
    } else {
        String::new()
    };
    code.push_str(&format!(
        "function __tuneka_indexed_transform(input){{var result={}({first},{second},{argument});if(typeof result!==\"string\")throw Error('player transform result type');{error_check}return result;}}",
        selected.name
    ));
    Ok(Some(Program {
        name: "__tuneka_indexed_transform".into(),
        source: code,
    }))
}
fn unsupported_indexed() -> Error {
    Error::new(
        ErrorCode::Unsupported,
        "indexed player transform layout or dependencies are unsupported",
    )
}

fn execute(program: &Program, input: &str) -> Result<String> {
    let runtime = Runtime::new()
        .map_err(|_| Error::new(ErrorCode::Internal, "cannot initialize player JS runtime"))?;
    // Use QuickJS's default allocator: custom allocator features would disable
    // this native heap quota. No loaders or host filesystem/network APIs exist.
    runtime.set_memory_limit(32 * 1024 * 1024);
    runtime.set_max_stack_size(512 * 1024);
    let deadline = Instant::now() + Duration::from_secs(2);
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
    let context = Context::full(&runtime)
        .map_err(|_| Error::new(ErrorCode::Internal, "cannot initialize player JS context"))?;
    let source = format!(
        "{}\n{}({});",
        program.source,
        program.name,
        serde_json::to_string(input)?
    );
    let result: String = context
        .with(|ctx| ctx.eval(source.as_bytes()))
        .map_err(|_| {
            Error::new(
                ErrorCode::Unsupported,
                "player transform failed or exceeded time, stack or memory limit",
            )
        })?;
    if result.is_empty() || result.len() > 8192 || result.starts_with("enhanced_except_") {
        return Err(Error::new(
            ErrorCode::Protocol,
            "invalid player transform result",
        ));
    }
    Ok(result)
}

/// Locate a brace/array expression while ignoring quoted strings and comments.
/// Unsupported syntax fails closed during the subsequent JS parse/evaluation.
fn balanced_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut stack = vec![];
    let mut quote = 0u8;
    let mut escaped = false;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut regex = false;
    let mut regex_class = false;
    let mut previous = 0u8;
    let mut i = start;
    while i < bytes.len() {
        let b = bytes[i];
        let next = bytes.get(i + 1).copied().unwrap_or(0);
        if line_comment {
            if b == b'\n' {
                line_comment = false;
            }
            i += 1;
            continue;
        }
        if block_comment {
            if b == b'*' && next == b'/' {
                block_comment = false;
                i += 1;
            }
            i += 1;
            continue;
        }
        if quote != 0 || regex {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if regex {
                if b == b'[' {
                    regex_class = true;
                } else if b == b']' {
                    regex_class = false;
                } else if b == b'/' && !regex_class {
                    regex = false;
                }
            } else if b == quote {
                quote = 0;
            }
            i += 1;
            continue;
        }
        if matches!(b, b'\'' | b'"' | b'`') {
            quote = b;
        } else if b == b'/' && next == b'/' {
            line_comment = true;
            i += 1;
        } else if b == b'/' && next == b'*' {
            block_comment = true;
            i += 1;
        } else if b == b'/' && matches!(previous, b'=' | b'(' | b':' | b',' | b'!' | b'[' | b';') {
            regex = true;
        } else if matches!(b, b'{' | b'[') {
            stack.push(b);
        } else if matches!(b, b'}' | b']') {
            let open = stack.pop()?;
            if (open == b'{') != (b == b'}') {
                return None;
            }
            if stack.is_empty() {
                return Some(i + 1);
            }
        }
        if !b.is_ascii_whitespace() {
            previous = b;
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn signature_helper_executes_without_browser_or_network() {
        let source = r#"var H={r:function(a){a.reverse()},w:function(a,b){var c=a[0];a[0]=a[b%a.length];a[b%a.length]=c}};S=function(a){a=a.split("");H.w(a,2);H.r(a);return a.join("")};x.sig||S(x);"#;
        let program = extract_program(source, false).unwrap();
        assert_eq!(execute(&program, "abcde").unwrap(), "edabc");
    }
    #[test]
    fn dollar_named_helper_is_a_real_dependency() {
        let source = r#"var $H={r:function(a){a.reverse()}};S=function(a){a=a.split("");$H.r(a);return a.join("")};x.sig||S(x);"#;
        let program = extract_program(source, false).unwrap();
        assert_eq!(execute(&program, "abc").unwrap(), "cba");
    }
    #[test]
    fn n_transform_and_failure_marker() {
        let program=extract_program(r#"N=function(a){if(a=="bad")return "enhanced_except_error";return a.split("").reverse().join("")};"#,true).unwrap();
        assert_eq!(execute(&program, "abcdef").unwrap(), "fedcba");
        assert!(execute(&program, "bad").is_err());
    }
    #[test]
    fn infinite_loop_is_bounded() {
        let program = Program {
            name: "N".into(),
            source: "function N(a){while(true){}return a}".into(),
        };
        assert!(execute(&program, "abc").is_err());
    }
    #[test]
    fn native_string_allocation_obeys_heap_limit() {
        let program = Program {
            name: "N".into(),
            source: "function N(a){return a.repeat(100000000)}".into(),
        };
        assert!(execute(&program, "abc").is_err());
    }
    #[test]
    fn cipher_uses_declared_signature_parameter() {
        let cipher = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("url", "https://rr1.googlevideo.com/videoplayback?itag=140")
            .append_pair("s", "abc")
            .append_pair("sp", "sig")
            .finish();
        let (_, signature) = stream_url(&json!({"signatureCipher":cipher})).unwrap();
        assert_eq!(signature, Some(("sig".into(), "abc".into())));
        assert!(stream_url(&json!({"itag":140})).is_err());
    }
    #[test]
    fn scanner_ignores_strings_comments_and_regex_braces() {
        let source = r#"{var a="}";/* } */var re=/[{}]/;return a;}tail"#;
        assert_eq!(
            &source[..balanced_end(source, 0).unwrap()],
            r#"{var a="}";/* } */var re=/[{}]/;return a;}"#
        );
    }
    #[test]
    fn current_indexed_player_executes_synthetic_signature_and_n() {
        // Google LLC / Closure Authors' Apache-2.0 public player definitions;
        // fixture retains upstream notices. Inputs are synthetic, never live tokens.
        let source = include_str!("../tests/fixtures/challenge/player-22cb88bc-indexed.js");
        let signature = extract_program(source, false).unwrap();
        let n = extract_program(source, true).unwrap();
        assert_eq!(
            execute(
                &signature,
                "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
            )
            .unwrap(),
            "M3210ZYXWVUTSRQPON4LKJIHGFED6BAzyxwvutsrqponmlkjihgfed"
        );
        assert_eq!(execute(&n, "abcdefghijklmnopq").unwrap(), "A24PpjEZNyCPBm");
        assert!(signature.source.len() < 131072 && n.source.len() < 131072);
    }
    #[test]
    fn current_plasma_player_executes_synthetic_signature() {
        let source =
            include_str!("../tests/fixtures/challenge/player-22cb88bc-plasma-signature.js");
        let signature = extract_program(source, false).unwrap();
        assert_eq!(
            execute(
                &signature,
                "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
            )
            .unwrap(),
            "M3210ZYXWVUTSRQPON4LKJIHGFED6BAzyxwvutsrqponmlkjihgfed"
        );
        assert!(signature.source.len() < 131072);
        assert!(extract_program(&source.replace("Mg(42,4042", "Mg(43,4042"), false).is_err());
    }
    #[test]
    fn direct_indexed_signature_executes_and_rejects_wrong_selector() {
        let source =
            include_str!("../tests/fixtures/challenge/player-21090930-direct-signature.js");
        let signature = extract_program(source, false).unwrap();
        assert_eq!(
            execute(
                &signature,
                "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
            )
            .unwrap(),
            "1fgdijklmnopqrstuvwxyzABCDEFGHeJhLMNOPQRSTUVWXYZ0a23I56789"
        );
        assert!(signature.source.len() < 131072);
        assert!(extract_program(&source.replace("zK(17,5531", "zK(18,5531"), false).is_err());
    }
    #[test]
    fn direct_indexed_n_is_tied_to_unique_url_setter_and_xor_binding() {
        let source = include_str!("../tests/fixtures/challenge/player-21090930-direct-n.js");
        let program = extract_program(source, true).unwrap();
        assert_eq!(execute(&program, "abcdef").unwrap(), "fedcba");
        assert!(extract_program(&format!("{source}U_=function(q,K,W,O,u){{}};"), true).is_err());
        assert!(extract_program(&source.replace("var u=K^q,J", "var u=K+q,J"), true).is_err());
        assert!(
            extract_program(
                &source.replace("O[I[J^1343]](I[J^1319],l)", "O[I[J^1343]](I[J^1318],l)"),
                true
            )
            .is_err()
        );
    }
    #[test]
    fn plasma_n_uses_nonzero_empty_table_entry_and_unique_setter() {
        let source = include_str!("../tests/fixtures/challenge/player-22cb88bc-plasma-n.js");
        let program = extract_program(source, true).unwrap();
        assert_eq!(execute(&program, "abcdef").unwrap(), "fedcba");
        assert!(extract_program(&format!("{source}My=function(r,A,W,h){{}};"), true).is_err());
        assert!(extract_program(&format!("{source}var DxV=0;"), true).is_err());
        assert!(extract_program(&source.replace("a[J^2686]", "a[J^2687]"), true).is_err());
        assert!(extract_program(&source.replace("a[J^3546]", "a[J^3547]"), true).is_err());
    }
    #[test]
    fn indexed_n_without_typeof_guard_constant_fails_closed() {
        let source = include_str!("../tests/fixtures/challenge/player-22cb88bc-indexed.js");
        let source = source.replace("var QhA=1886947328;", "");
        assert!(extract_program(&source, true).is_err());
    }
    #[test]
    fn ambiguous_signature_functions_are_rejected() {
        let source = r#"A=function(a){return a.split("").reverse().join("")};B=function(a){return a.split("").slice(1).join("")};"#;
        assert!(extract_program(source, false).is_err());
        let source = format!("{source}x.sig||Missing(x);");
        assert!(extract_program(&source, false).is_err());
    }
    #[test]
    fn indexed_duplicate_entry_is_rejected() {
        let source = include_str!("../tests/fixtures/challenge/player-22cb88bc-indexed.js");
        let source = format!("{source}var Ko=function(r,A,W){{return W;}};");
        assert!(extract_program(&source, true).is_err());
    }
    #[test]
    fn indexed_n_error_marker_is_not_a_valid_challenge_result() {
        let source = include_str!("../tests/fixtures/challenge/player-22cb88bc-indexed.js");
        let marker = "if(typeof QhA===d[J^3533]){C=W;break a}try{try{";
        assert!(source.contains(marker));
        let source = source.replacen(
            marker,
            "if(typeof QhA===d[J^3533]){C=W;break a}try{throw new Error('forced');try{",
            1,
        );
        let program = extract_program(&source, true).unwrap();
        assert!(execute(&program, "abcdefghijklmnopq").is_err());
    }
    #[test]
    fn indexed_signature_ignores_same_key_calls_to_other_dispatch_branches() {
        let source = include_str!("../tests/fixtures/challenge/player-22cb88bc-indexed.js");
        let source = format!("var decoy=T2(0,4064,'not a signature branch');{source}");
        let program = extract_program(&source, false).unwrap();
        assert_eq!(
            execute(
                &program,
                "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
            )
            .unwrap(),
            "M3210ZYXWVUTSRQPON4LKJIHGFED6BAzyxwvutsrqponmlkjihgfed"
        );
    }
    #[test]
    fn oversized_nested_browser_definitions_are_not_cloned() {
        let payload = "x".repeat(MAX_DEFINITION_BYTES);
        let source = format!(
            "Outer=function(){{Inner=function(){{return '{payload}'}};}};Small=function(s){{return s.split(\"\").reverse().join(\"\")}};"
        );
        let definitions = functions(&source).unwrap();
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].name, "Small");
        assert!(definitions[0].source.len() < MAX_DEFINITION_BYTES);
        assert_eq!(
            execute(&extract_program(&source, false).unwrap(), "abc").unwrap(),
            "cba"
        );
    }
    #[test]
    fn cumulative_definition_copy_budget_is_enforced() {
        let payload = "x".repeat(120_000);
        let source = (0..71)
            .map(|i| format!("F{i}=function(){{return '{payload}'}};"))
            .collect::<String>();
        assert!(source.len() < 10 * 1024 * 1024);
        assert!(functions(&source).is_err());
    }
    #[test]
    fn oversized_indexed_string_tables_are_rejected() {
        let mut table = ";split;join;signatureCipher;n;".to_owned();
        table.push_str(&"x;".repeat(100_000));
        let source = format!(
            "var d={}.split(\";\");",
            serde_json::to_string(&table).unwrap()
        );
        assert!(extract_program(&source, true).is_err());
    }
}
