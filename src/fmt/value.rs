//! 按指定格式把字节解码为显示文本。

use super::{guess, DecodeMode, Endian};

const EMPTY: &str = "（空）";
const PAD_NOTE: &str = "（补零）";

/// 按 mode 解码 bytes，多字节整数按 endian 解释，
/// 结果最多 max_chars 个字符（超出截断并加省略号）。
///
/// 字节数少于类型宽度时按零扩展补齐（小端补尾部、大端补头部，数值等价），
/// 并在结果后标注"（补零）"；空数据显示"（空）"；超长取前 N 字节。
pub fn decode(bytes: &[u8], mode: DecodeMode, endian: Endian, max_chars: usize) -> String {
    // 整数分支：零扩展补齐。返回 (文本, 是否补齐)
    macro_rules! int {
        ($ty:ty) => {{
            let n = std::mem::size_of::<$ty>();
            if bytes.is_empty() {
                EMPTY.to_string()
            } else {
                let take = bytes.len().min(n);
                let mut arr = [0u8; { std::mem::size_of::<$ty>() }];
                match endian {
                    Endian::Little => arr[..take].copy_from_slice(&bytes[..take]),
                    Endian::Big => arr[n - take..].copy_from_slice(&bytes[..take]),
                }
                let v = match endian {
                    Endian::Little => <$ty>::from_le_bytes(arr),
                    Endian::Big => <$ty>::from_be_bytes(arr),
                };
                if bytes.len() < n {
                    format!("{v}{PAD_NOTE}")
                } else {
                    v.to_string()
                }
            }
        }};
    }

    // 浮点分支：同样零扩展补齐
    macro_rules! float {
        ($ty:ty) => {{
            let n = std::mem::size_of::<$ty>();
            if bytes.is_empty() {
                EMPTY.to_string()
            } else {
                let take = bytes.len().min(n);
                let mut arr = [0u8; { std::mem::size_of::<$ty>() }];
                match endian {
                    Endian::Little => arr[..take].copy_from_slice(&bytes[..take]),
                    Endian::Big => arr[n - take..].copy_from_slice(&bytes[..take]),
                }
                let v = match endian {
                    Endian::Little => <$ty>::from_le_bytes(arr).to_string(),
                    Endian::Big => <$ty>::from_be_bytes(arr).to_string(),
                };
                if bytes.len() < n {
                    format!("{v}{PAD_NOTE}")
                } else {
                    v
                }
            }
        }};
    }

    let s = match mode {
        DecodeMode::Auto => guess(bytes, endian).1,
        DecodeMode::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        DecodeMode::Utf16Le => utf16_text(bytes, false),
        DecodeMode::Utf16Be => utf16_text(bytes, true),
        DecodeMode::I8 => int!(i8),
        DecodeMode::U8 => int!(u8),
        DecodeMode::I16 => int!(i16),
        DecodeMode::I32 => int!(i32),
        DecodeMode::I64 => int!(i64),
        DecodeMode::U16 => int!(u16),
        DecodeMode::U32 => int!(u32),
        DecodeMode::U64 => {
            if bytes.is_empty() {
                EMPTY.to_string()
            } else {
                let n = 8;
                let take = bytes.len().min(n);
                let mut arr = [0u8; 8];
                match endian {
                    Endian::Little => arr[..take].copy_from_slice(&bytes[..take]),
                    Endian::Big => arr[n - take..].copy_from_slice(&bytes[..take]),
                }
                let v = match endian {
                    Endian::Little => u64::from_le_bytes(arr),
                    Endian::Big => u64::from_be_bytes(arr),
                };
                if bytes.len() < n {
                    format!("{}{PAD_NOTE}", u64_text(v))
                } else {
                    u64_text(v)
                }
            }
        }
        DecodeMode::F32 => float!(f32),
        DecodeMode::F64 => float!(f64),
        DecodeMode::Hex => hex_spaced(bytes),
        DecodeMode::Dec => dec_spaced(bytes),
        DecodeMode::Binary => binary_bits(bytes),
    };
    truncate_chars(s, max_chars)
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

/// 每字节显示为 8 位二进制，空格分隔。
fn binary_bits(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:08b}"))
        .collect::<Vec<_>>()
        .join(" ")
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
