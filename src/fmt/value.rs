//! 按指定格式把字节解码为显示文本。

use super::{guess, DecodeMode};

const LEN_MISMATCH: &str = "（长度不符）";

/// 按 mode 解码 bytes，结果最多 max_chars 个字符（超出截断并加省略号）。
pub fn decode(bytes: &[u8], mode: DecodeMode, max_chars: usize) -> String {
    let s = match mode {
        DecodeMode::Auto => guess(bytes).1,
        DecodeMode::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        DecodeMode::Utf16Le => utf16_text(bytes, false),
        DecodeMode::Utf16Be => utf16_text(bytes, true),
        DecodeMode::I8 => need(bytes, 1).map_or(LEN_MISMATCH.into(), |b| (b[0] as i8).to_string()),
        DecodeMode::I16 => need(bytes, 2)
            .map_or(LEN_MISMATCH.into(), |b| {
                i16::from_le_bytes(b.try_into().unwrap()).to_string()
            }),
        DecodeMode::I32 => need(bytes, 4)
            .map_or(LEN_MISMATCH.into(), |b| {
                i32::from_le_bytes(b.try_into().unwrap()).to_string()
            }),
        DecodeMode::I64 => need(bytes, 8)
            .map_or(LEN_MISMATCH.into(), |b| {
                i64::from_le_bytes(b.try_into().unwrap()).to_string()
            }),
        DecodeMode::U8 => need(bytes, 1).map_or(LEN_MISMATCH.into(), |b| b[0].to_string()),
        DecodeMode::U16 => need(bytes, 2)
            .map_or(LEN_MISMATCH.into(), |b| {
                u16::from_le_bytes(b.try_into().unwrap()).to_string()
            }),
        DecodeMode::U32 => need(bytes, 4)
            .map_or(LEN_MISMATCH.into(), |b| {
                u32::from_le_bytes(b.try_into().unwrap()).to_string()
            }),
        DecodeMode::U64 => need(bytes, 8)
            .map_or(LEN_MISMATCH.into(), |b| {
                u64_text(u64::from_le_bytes(b.try_into().unwrap()))
            }),
        DecodeMode::F32 => need(bytes, 4)
            .map_or(LEN_MISMATCH.into(), |b| {
                f32::from_le_bytes(b.try_into().unwrap()).to_string()
            }),
        DecodeMode::F64 => need(bytes, 8)
            .map_or(LEN_MISMATCH.into(), |b| {
                f64::from_le_bytes(b.try_into().unwrap()).to_string()
            }),
        DecodeMode::Hex => hex_spaced(bytes),
        DecodeMode::Dec => dec_spaced(bytes),
        DecodeMode::Binary => binary_escaped(bytes),
    };
    truncate_chars(s, max_chars)
}

fn need(bytes: &[u8], n: usize) -> Option<&[u8]> {
    (bytes.len() >= n).then_some(&bytes[..n])
}

/// 空格分隔的大写十六进制。
pub fn hex_spaced(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn dec_spaced(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// 可打印 ASCII 原样显示，其余转义为 \xNN。
fn binary_escaped(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        if b.is_ascii_graphic() || b == b' ' {
            out.push(b as char);
        } else {
            out.push_str(&format!("\\x{b:02X}"));
        }
    }
    out
}

fn utf16_text(bytes: &[u8], big_endian: bool) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| {
            if big_endian {
                u16::from_be_bytes([c[0], c[1]])
            } else {
                u16::from_le_bytes([c[0], c[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

/// u64 显示文本；落在 Unix 时间戳合理区间时附本地日期。
pub fn u64_text(v: u64) -> String {
    match epoch_to_local(v) {
        Some(date) => format!("{v} → {date}"),
        None => v.to_string(),
    }
}

/// 识别秒（1e9..1e10）或毫秒（1e12..1e13）级 Unix 时间戳，格式化为本地时间。
fn epoch_to_local(v: u64) -> Option<String> {
    let secs = if (1_000_000_000..10_000_000_000).contains(&v) {
        v as i64
    } else if (1_000_000_000_000..10_000_000_000_000).contains(&v) {
        (v / 1000) as i64
    } else {
        return None;
    };
    let dt = chrono::DateTime::from_timestamp(secs, 0)?;
    let local = dt.with_timezone(&chrono::Local);
    Some(local.format("%Y-%m-%d %H:%M:%S (%Z%:z)").to_string())
}

fn truncate_chars(s: String, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s;
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push('…');
    out
}
