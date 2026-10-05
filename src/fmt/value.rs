// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

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
        DecodeMode::Base64 => base64_text(bytes),
        DecodeMode::Uuid => uuid_text(bytes),
        DecodeMode::Json => json_text(bytes),
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

/// RFC 4648 标准 Base64 编码（含填充）。
fn base64_text(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let mut buf = [0u8; 3];
        for (i, &b) in chunk.iter().enumerate() {
            buf[i] = b;
        }
        out.push(TABLE[(buf[0] >> 2) as usize] as char);
        out.push(TABLE[(((buf[0] & 0x03) << 4) | (buf[1] >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(((buf[1] & 0x0F) << 2) | (buf[2] >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(buf[2] & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// UUID：仅 16 字节时格式化为 8-4-4-4-12 小写 hex；否则回退 hex_spaced。
fn uuid_text(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    if bytes.len() != 16 {
        let t = crate::i18n::tr();
        return format!("{} {}", hex_spaced(bytes), t.uuid_bad_len);
    }
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11],
        bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

/// JSON 美化：合法 JSON → pretty；非法 → UTF-8 文本。
fn json_text(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    match serde_json::from_slice::<serde_json::Value>(bytes) {
        Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned()),
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fmt::{DecodeMode, Endian, set_thousands_sep};

    fn setup() {
        // 测试统一打开千位分隔，避免全局状态污染断言
        set_thousands_sep(true);
    }

    #[test]
    fn hex_spaced_basic() {
        assert_eq!(hex_spaced(&[0x2A, 0x2B, 0x2C]), "2A 2B 2C");
        assert_eq!(hex_spaced(&[]), "");
        assert_eq!(hex_spaced(&[0x00, 0xFF]), "00 FF");
    }

    #[test]
    fn decode_utf8() {
        setup();
        let s = decode(b"hello", DecodeMode::Utf8, Endian::Little, 100);
        assert_eq!(s, "hello");
        // 非 UTF-8 字节走 lossy
        let s = decode(&[0xFF, 0xFE], DecodeMode::Utf8, Endian::Little, 100);
        assert!(s.contains('\u{FFFD}'));
    }

    #[test]
    fn decode_u8() {
        setup();
        assert_eq!(decode(&[255], DecodeMode::U8, Endian::Little, 100), "255");
        assert_eq!(decode(&[128], DecodeMode::I8, Endian::Little, 100), "-128");
    }

    #[test]
    fn decode_u16_le_be() {
        setup();
        let bytes = [0x34, 0x12]; // LE=0x1234(4660), BE=0x3412(13330)
        // 4 位数不加千位分隔
        assert_eq!(decode(&bytes, DecodeMode::U16, Endian::Little, 100), "4660");
        assert_eq!(decode(&bytes, DecodeMode::U16, Endian::Big, 100), "13,330");
    }

    #[test]
    fn decode_u32_le() {
        setup();
        let bytes = [0x78, 0x56, 0x34, 0x12]; // 0x12345678
        assert_eq!(decode(&bytes, DecodeMode::U32, Endian::Little, 100), "305,419,896");
    }

    #[test]
    fn decode_u64_le() {
        setup();
        let bytes = [0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(decode(&bytes, DecodeMode::U64, Endian::Little, 100), "1");
    }

    #[test]
    fn decode_i32_negative() {
        setup();
        let bytes = (-1i32).to_le_bytes();
        assert_eq!(decode(&bytes, DecodeMode::I32, Endian::Little, 100), "-1");
    }

    #[test]
    fn decode_short_bytes_zero_extend() {
        setup();
        // 1 字节当 u32 解：小端补尾部零 → 数值不变，标注补零
        let s = decode(&[0x05], DecodeMode::U32, Endian::Little, 100);
        assert!(s.starts_with("5"));
        assert!(s.ends_with(crate::i18n::tr().padded));
        // 大端补头部零 → 同样数值 5
        let s = decode(&[0x05], DecodeMode::U32, Endian::Big, 100);
        assert!(s.starts_with("5"));
    }

    #[test]
    fn decode_empty() {
        setup();
        let t = crate::i18n::tr();
        use DecodeMode::*;
        // 整数/浮点模式：空数据返回通用空占位文案
        let numeric = [I8, U8, I16, U16, I32, U32, I64, U64, F32, F64];
        for mode in numeric {
            let s = decode(&[], mode, Endian::Little, 100);
            assert_eq!(s, t.empty, "mode={:?}", mode);
        }
        // 字符串/进制模式：空数据返回空串（无内容可展示）
        let textual = [Utf8, Utf16Le, Utf16Be, Hex, Dec, Binary];
        for mode in textual {
            let s = decode(&[], mode, Endian::Little, 100);
            assert_eq!(s, "", "mode={:?}", mode);
        }
        // Auto 模式：走 guess，返回 guess_empty_sym
        let s = decode(&[], Auto, Endian::Little, 100);
        assert_eq!(s, t.guess_empty_sym);
    }

    #[test]
    fn decode_hex_mode() {
        setup();
        assert_eq!(decode(&[0xAB, 0xCD], DecodeMode::Hex, Endian::Little, 100), "AB CD");
    }

    #[test]
    fn decode_dec_mode() {
        setup();
        assert_eq!(decode(&[10, 20], DecodeMode::Dec, Endian::Little, 100), "10 20");
    }

    #[test]
    fn decode_binary_mode() {
        setup();
        assert_eq!(decode(&[0b1010_1010], DecodeMode::Binary, Endian::Little, 100), "10101010");
    }

    #[test]
    fn decode_truncate_chars() {
        setup();
        let long = "a".repeat(50);
        let s = decode(long.as_bytes(), DecodeMode::Utf8, Endian::Little, 10);
        assert_eq!(s.chars().count(), 11); // 10 字符 + 省略号
        assert!(s.ends_with('…'));
    }

    #[test]
    fn with_sep_integer() {
        set_thousands_sep(true);
        assert_eq!(with_sep("1625981420".to_string()), "1,625,981,420");
        assert_eq!(with_sep("-1625981420".to_string()), "-1,625,981,420");
        // 少于 5 位不加
        assert_eq!(with_sep("1234".to_string()), "1234");
        // 关闭开关
        set_thousands_sep(false);
        assert_eq!(with_sep("1625981420".to_string()), "1625981420");
        set_thousands_sep(true);
    }

    #[test]
    fn with_sep_float_only_int_part() {
        set_thousands_sep(true);
        // 整数部分 < 5 位，原样
        assert_eq!(with_sep("3.14".to_string()), "3.14");
        // 指数后缀保留
        assert_eq!(with_sep("1.5e10".to_string()), "1.5e10");
    }

    #[test]
    fn u64_text_timestamp() {
        setup();
        // 秒级时间戳：2024-01-01 00:00:00 UTC
        let v = 1_704_067_200u64;
        let s = u64_text(v);
        assert!(s.starts_with("1,704,067,200 → "));
        assert!(s.contains("2024"));
        // 非时间戳范围：只显示数字
        let s = u64_text(42);
        assert_eq!(s, "42");
    }

    #[test]
    fn decode_f32() {
        setup();
        let bytes = 1.5f32.to_le_bytes();
        let s = decode(&bytes, DecodeMode::F32, Endian::Little, 100);
        assert_eq!(s, "1.5");
    }

    #[test]
    fn decode_f64() {
        setup();
        let bytes = 2.5f64.to_le_bytes();
        let s = decode(&bytes, DecodeMode::F64, Endian::Little, 100);
        assert_eq!(s, "2.5");
    }

    #[test]
    fn decode_utf16_le() {
        setup();
        // "AB" 的 UTF-16LE
        let bytes = [0x41, 0x00, 0x42, 0x00];
        assert_eq!(decode(&bytes, DecodeMode::Utf16Le, Endian::Little, 100), "AB");
    }

    #[test]
    fn decode_utf16_be() {
        setup();
        let bytes = [0x00, 0x41, 0x00, 0x42];
        assert_eq!(decode(&bytes, DecodeMode::Utf16Be, Endian::Little, 100), "AB");
    }

    #[test]
    fn decode_base64() {
        setup();
        assert_eq!(decode(b"hello", DecodeMode::Base64, Endian::Little, 100), "aGVsbG8=");
        assert_eq!(decode(b"f", DecodeMode::Base64, Endian::Little, 100), "Zg==");
        assert_eq!(decode(b"fo", DecodeMode::Base64, Endian::Little, 100), "Zm8=");
        assert_eq!(decode(b"foo", DecodeMode::Base64, Endian::Little, 100), "Zm9v");
        assert_eq!(decode(&[], DecodeMode::Base64, Endian::Little, 100), "");
    }

    #[test]
    fn decode_uuid_16_bytes() {
        setup();
        let bytes: [u8; 16] = [
            0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4,
            0xa7, 0x16, 0x44, 0x66, 0x55, 0x44, 0x00, 0x00,
        ];
        assert_eq!(
            decode(&bytes, DecodeMode::Uuid, Endian::Little, 100),
            "550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn decode_uuid_wrong_len() {
        setup();
        let t = crate::i18n::tr();
        // 15 字节：回退 hex + 标注
        let bytes = [0xABu8; 15];
        let s = decode(&bytes, DecodeMode::Uuid, Endian::Little, 500);
        assert!(s.contains("AB AB"));
        assert!(s.contains(t.uuid_bad_len));
        // 17 字节同样回退
        let bytes = [0x01u8; 17];
        assert!(decode(&bytes, DecodeMode::Uuid, Endian::Little, 500).contains(t.uuid_bad_len));
        // 空输入为空串
        assert_eq!(decode(&[], DecodeMode::Uuid, Endian::Little, 100), "");
    }

    #[test]
    fn decode_json_pretty() {
        setup();
        let s = decode(br#"{"a":1,"b":[2,3]}"#, DecodeMode::Json, Endian::Little, 500);
        assert!(s.contains('\n'), "pretty JSON 应多行: {s}");
        assert!(s.contains("\"a\": 1"));
        // 非法 JSON 回退 UTF-8 文本
        let s = decode(b"not json", DecodeMode::Json, Endian::Little, 100);
        assert_eq!(s, "not json");
        // 空输入为空串
        assert_eq!(decode(&[], DecodeMode::Json, Endian::Little, 100), "");
    }
}
