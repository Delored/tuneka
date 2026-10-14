//! Explicit local challenge diagnostic. Inputs/outputs remain in caller-selected
//! transient files; stdout contains only lengths and success status.
#[allow(dead_code)]
#[path = "../src/player.rs"]
mod player;
use std::{io::Read, sync::Arc};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string(
        std::env::args()
            .nth(1)
            .ok_or("missing player source path")?,
    )?;
    let output = std::env::args()
        .nth(2)
        .ok_or("missing transient result path")?;
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let input: serde_json::Value = serde_json::from_str(&input)?;
    let mut results = serde_json::Map::new();
    for (key, n) in [("signature", false), ("n", true)] {
        if let Some(value) = input[key].as_str() {
            let started = std::time::Instant::now();
            match player::transform(
                Arc::new(source.clone()),
                value.into(),
                n,
                Arc::new(tokio::sync::Semaphore::new(1)),
            )
            .await
            {
                Ok(transformed) => {
                    println!(
                        "{}",
                        serde_json::json!({"kind":key,"success":true,"input_length":value.len(),"output_length":transformed.len(),"changed":value!=transformed,"elapsed_ms":started.elapsed().as_millis()})
                    );
                    results.insert(key.into(), transformed.into());
                }
                Err(error) => {
                    println!(
                        "{}",
                        serde_json::json!({"kind":key,"success":false,"error":error.to_string()})
                    );
                }
            }
        }
    }
    std::fs::write(output, serde_json::to_vec(&results)?)?;
    Ok(())
}
