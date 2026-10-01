//! 按指定格式把字节解码为显示文本。

use super::{guess, DecodeMode, Endian};

/// 空数据占位文案（随界面语言）。
fn empty_text() -> String {
    crate::i18n::tr().empty.to_string()
}

/// 补零标注（随界面语言）。
fn pad_note() -> &'static str {
    crate::i18n::tr().padded
}

/// 给数字字符串加千位分隔符（如 `1625981420` → `1,625,981,420`，
/// `1.62598142e20` → `1.62598142e20` 整数部分不变（太短）等）。
/// 受全局开关控制，关闭时原样返回。
pub(crate) fn with_sep(s: String) -> String {
    if !super::thousands_sep() {
        return s;
    }
    // 浮点数：只给整数部分加逗号，小数/指数后缀保留
    if let Some(pos) = s.find(['.', 'e', 'E']) {
        let int_part = &s[..pos];
        let suffix = &s[pos..];
        return format!("{}{}", sep_int(int_part), suffix);
    }
    // 纯整数（含负号）
    sep_int(&s)
}

/// 给纯整数部分（可带负号）加千位分隔符；非纯数字或太短则原样返回。
fn sep_int(s: &str) -> String {
    let (neg, digits) = match s.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", s),
    };
    if digits.len() < 5 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + digits.len() / 3);
    out.push_str(neg);
    let rem = digits.len() % 3;
    for (i, c) in digits.char_indices() {
        if i > 0 && i % 3 == rem {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// 按 mode 解码 bytes，多字节整数按 endian 解释，
/// 结果最多 max_chars 个字符（超出截断并加省略号）。
///
/// 字节数少于类型宽度时按零扩展补齐（小端补尾部、大端补头部，数值等价），
/// 并在结果后标注补零说明；空数据显示空占位；超长取前 N 字节。
/// 占位文案随当前界面语言。
pub fn decode(bytes: &[u8], mode: DecodeMode, endian: Endian, max_chars: usize) -> String {
    // 整数分支：零扩展补齐。返回 (文本, 是否补齐)
    macro_rules! int {
        ($ty:ty) => {{
            let n = std::mem::size_of::<$ty>();
            if bytes.is_empty() {
                empty_text()
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
                    format!("{}{}", with_sep(v.to_string()), pad_note())
                } else {
                    with_sep(v.to_string())
                }
            }
        }};
    }

    // 浮点分支：同样零扩展补齐
    macro_rules! float {
        ($ty:ty) => {{
            let n = std::mem::size_of::<$ty>();
            if bytes.is_empty() {
                empty_text()
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
                // Rust 浮点 Display 不用科学计数法：极小值（如补零后的 denormal）
                // 会展开成几百位 0.000…，超长时改用科学计数法
                let s = format!("{v}");
                let s = if s.len() > 24 { format!("{v:e}") } else { s };
                if bytes.len() < n {
                    format!("{}{}", with_sep(s), pad_note())
                } else {
                    with_sep(s)
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
                empty_text()
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
                    format!("{}{}", u64_text(v), pad_note())
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

/// 空格分隔的大写十六进制（如 `2A 2B 2C`）。
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
/// 毫秒级值保留 3 位小数（`.xxx`），秒级值只到秒。数字部分带千位分隔。
pub fn u64_text(v: u64) -> String {
    let num = with_sep(v.to_string());
    match epoch_to_local(v) {
        Some(date) => format!("{num} → {date}"),
        None => num,
    }
}

/// 识别秒（1e9..1e10）或毫秒（1e12..1e13）级 Unix 时间戳，格式化为本地时间；
/// 毫秒级保留并显示 3 位毫秒（`.000`），秒级只到秒。
fn epoch_to_local(v: u64) -> Option<String> {
    if (1_000_000_000..10_000_000_000).contains(&v) {
        let dt = chrono::DateTime::from_timestamp(v as i64, 0)?;
        Some(
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S (%Z%:z)")
                .to_string(),
        )
    } else if (1_000_000_000_000..10_000_000_000_000).contains(&v) {
        // 毫秒：余数转成纳秒传入，格式串里 %.3f 显示 .xxx
        let dt = chrono::DateTime::from_timestamp(
            (v / 1000) as i64,
            (v % 1000 * 1_000_000) as u32,
        )?;
        Some(
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S%.3f (%Z%:z)")
                .to_string(),
        )
    } else {
        None
    }
}

fn truncate_chars(s: String, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s;
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push('…');
    out
}
