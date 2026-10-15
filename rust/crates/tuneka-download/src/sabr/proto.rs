//! Minimal bounded protobuf and UMP wire codec, implemented independently.
//! Field meanings reference LuanRT/googlevideo (MIT), revision
//! 96d1418984d00ac2797a1dafe4625d1368b5b9aa. No generated upstream code is embedded.
use tuneka_core::{Error, ErrorCode, Result};

fn invalid(message: &str) -> Error {
    Error::new(ErrorCode::Protocol, message)
}
pub(super) fn uint(out: &mut Vec<u8>, field: u32, value: u64) {
    varint(out, u64::from(field) << 3);
    varint(out, value);
}
pub(super) fn bytes(out: &mut Vec<u8>, field: u32, value: &[u8]) {
    varint(out, (u64::from(field) << 3) | 2);
    varint(out, value.len() as u64);
    out.extend_from_slice(value);
}
pub(super) fn float(out: &mut Vec<u8>, field: u32, value: f32) {
    varint(out, (u64::from(field) << 3) | 5);
    out.extend_from_slice(&value.to_le_bytes());
}
fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value > 127 {
        out.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    out.push(value as u8);
}
fn read_varint(data: &[u8], offset: &mut usize) -> Result<u64> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let byte = *data
            .get(*offset)
            .ok_or_else(|| invalid("truncated SABR protobuf integer"))?;
        *offset += 1;
        if shift == 63 && byte > 1 {
            return Err(invalid("SABR protobuf integer overflow"));
        }
        value |= u64::from(byte & 127) << shift;
        if byte < 128 {
            return Ok(value);
        }
    }
    Err(invalid("SABR protobuf integer overflow"))
}
#[derive(Clone, Copy)]
enum Value<'a> {
    Uint(u64),
    Bytes(&'a [u8]),
    Fixed,
}
pub(super) struct Message<'a> {
    fields: Vec<(u32, Value<'a>)>,
}
impl<'a> Message<'a> {
    pub(super) fn parse(data: &'a [u8]) -> Result<Self> {
        if data.len() > 128 * 1024 {
            return Err(invalid("SABR metadata size limit exceeded"));
        }
        let mut fields = Vec::new();
        let mut offset = 0;
        while offset < data.len() {
            if fields.len() >= 4096 {
                return Err(invalid("SABR protobuf field limit exceeded"));
            }
            let tag = read_varint(data, &mut offset)?;
            let field = u32::try_from(tag >> 3)
                .ok()
                .filter(|n| *n > 0 && *n < (1 << 29))
                .ok_or_else(|| invalid("invalid SABR protobuf field"))?;
            let value = match tag & 7 {
                0 => Value::Uint(read_varint(data, &mut offset)?),
                2 => {
                    let length = usize::try_from(read_varint(data, &mut offset)?)
                        .map_err(|_| invalid("SABR protobuf length overflow"))?;
                    let end = offset
                        .checked_add(length)
                        .filter(|n| *n <= data.len())
                        .ok_or_else(|| invalid("truncated SABR protobuf bytes"))?;
                    let value = Value::Bytes(&data[offset..end]);
                    offset = end;
                    value
                }
                wire @ (1 | 5) => {
                    let length = if wire == 1 { 8 } else { 4 };
                    offset = offset
                        .checked_add(length)
                        .filter(|n| *n <= data.len())
                        .ok_or_else(|| invalid("truncated SABR protobuf fixed value"))?;
                    Value::Fixed
                }
                _ => return Err(invalid("unsupported SABR protobuf wire type")),
            };
            fields.push((field, value));
        }
        Ok(Self { fields })
    }
    fn scalar(&self, field: u32) -> Result<Option<Value<'a>>> {
        let mut values = self.fields.iter().filter(|(number, _)| *number == field);
        let value = values.next().map(|(_, value)| *value);
        if values.next().is_some() {
            return Err(invalid("duplicate SABR protobuf scalar"));
        }
        Ok(value)
    }
    pub(super) fn uint(&self, field: u32) -> Result<Option<u64>> {
        match self.scalar(field)? {
            Some(Value::Uint(value)) => Ok(Some(value)),
            None => Ok(None),
            _ => Err(invalid("invalid SABR protobuf integer type")),
        }
    }
    pub(super) fn bytes(&self, field: u32) -> Result<Option<&'a [u8]>> {
        match self.scalar(field)? {
            Some(Value::Bytes(value)) => Ok(Some(value)),
            None => Ok(None),
            _ => Err(invalid("invalid SABR protobuf byte type")),
        }
    }
    pub(super) fn text(&self, field: u32) -> Result<Option<&'a str>> {
        self.bytes(field)?
            .map(|value| {
                std::str::from_utf8(value).map_err(|_| invalid("invalid SABR protobuf UTF-8"))
            })
            .transpose()
    }
    pub(super) fn uints(&self, field: u32) -> Result<Vec<u32>> {
        let mut result = Vec::new();
        for (_, value) in self.fields.iter().filter(|(number, _)| *number == field) {
            match value {
                Value::Uint(value) => result.push(
                    u32::try_from(*value).map_err(|_| invalid("SABR context type overflow"))?,
                ),
                Value::Bytes(data) => {
                    let mut offset = 0;
                    while offset < data.len() {
                        result.push(
                            u32::try_from(read_varint(data, &mut offset)?)
                                .map_err(|_| invalid("SABR context type overflow"))?,
                        );
                        if result.len() > 64 {
                            return Err(invalid("SABR context count limit exceeded"));
                        }
                    }
                }
                _ => return Err(invalid("invalid SABR context type")),
            }
            if result.len() > 64 {
                return Err(invalid("SABR context count limit exceeded"));
            }
        }
        Ok(result)
    }
}

/// UMP integers are not protobuf varints. Incomplete framing is retryable.
pub(super) fn ump_uint(data: &[u8], offset: &mut usize) -> Result<u32> {
    let byte = *data.get(*offset).ok_or_else(truncated)?;
    let length = match byte {
        0..=127 => 1,
        128..=191 => 2,
        192..=223 => 3,
        224..=239 => 4,
        240..=247 => 5,
        _ => return Err(invalid("invalid UMP integer prefix")),
    };
    let end = offset
        .checked_add(length)
        .filter(|n| *n <= data.len())
        .ok_or_else(truncated)?;
    let mut value = if length == 5 {
        0
    } else {
        u32::from(byte & (0xff >> length))
    };
    let shift = if length == 5 { 0 } else { 8 - length };
    for (index, byte) in data[*offset + 1..end].iter().enumerate() {
        value |= u32::from(*byte) << (shift + index * 8);
    }
    *offset = end;
    Ok(value)
}
fn truncated() -> Error {
    Error::new(ErrorCode::Network, "truncated SABR UMP response")
}
pub(super) fn parts(data: &[u8]) -> Result<Vec<(u32, &[u8])>> {
    let mut result = Vec::new();
    let mut offset = 0;
    while offset < data.len() {
        if result.len() >= 20_000 {
            return Err(invalid("SABR UMP part count limit exceeded"));
        }
        let kind = ump_uint(data, &mut offset)?;
        let length = ump_uint(data, &mut offset)? as usize;
        let end = offset
            .checked_add(length)
            .filter(|n| *n <= data.len())
            .ok_or_else(truncated)?;
        result.push((kind, &data[offset..end]));
        offset = end;
    }
    Ok(result)
}

#[cfg(test)]
pub(super) fn frame(kind: u32, payload: &[u8]) -> Vec<u8> {
    fn encode(out: &mut Vec<u8>, value: u32) {
        let (length, prefix, bits) = if value < 128 {
            (1, 0, 7)
        } else if value < 16384 {
            (2, 128, 6)
        } else if value < 2097152 {
            (3, 192, 5)
        } else if value < 268435456 {
            (4, 224, 4)
        } else {
            (5, 240, 0)
        };
        out.push(prefix | (value as u8 & ((1 << bits) - 1)));
        let rest = value >> bits;
        for index in 0..length - 1 {
            out.push((rest >> (index * 8)) as u8);
        }
    }
    let mut result = Vec::new();
    encode(&mut result, kind);
    encode(&mut result, payload.len() as u32);
    result.extend_from_slice(payload);
    result
}
