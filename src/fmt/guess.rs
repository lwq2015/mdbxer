// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 字节内容的自动类型猜测。

use super::{value, Endian};

/// 猜测字节内容的类型，返回 (类型标签, 显示文本)。
/// 多字节整数按 endian 解释。标签随当前界面语言。
pub fn guess(bytes: &[u8], endian: Endian) -> (String, String) {
    let t = crate::i18n::tr();
    if bytes.is_empty() {
        return (t.guess_empty_label.to_string(), t.guess_empty_sym.to_string());
    }
    // 优先可打印 UTF-8 文本（允许 \t \n \r）
    if let Ok(s) = std::str::from_utf8(bytes) {
        if s.chars()
            .all(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r'))
        {
            return (t.guess_utf8.to_string(), s.to_string());
        }
    }
    let sfx = endian.suffix();
    // match 保证长度后 try_into 必然成功
    match bytes.len() {
        8 => {
            let arr: [u8; 8] = bytes.try_into().unwrap_or_else(|_| unreachable!());
            let v = match endian {
                Endian::Little => u64::from_le_bytes(arr),
                Endian::Big => u64::from_be_bytes(arr),
            };
            (format!("u64 {sfx}"), value::u64_text(v))
        }
        4 => {
            let arr: [u8; 4] = bytes.try_into().unwrap_or_else(|_| unreachable!());
            let v = match endian {
                Endian::Little => u32::from_le_bytes(arr),
                Endian::Big => u32::from_be_bytes(arr),
            };
            (format!("u32 {sfx}"), value::with_sep(v.to_string()))
        }
        2 => {
            let arr: [u8; 2] = bytes.try_into().unwrap_or_else(|_| unreachable!());
            let v = match endian {
                Endian::Little => u16::from_le_bytes(arr),
                Endian::Big => u16::from_be_bytes(arr),
            };
            (format!("u16 {sfx}"), value::with_sep(v.to_string()))
        }
        _ => (t.guess_binary.to_string(), value::hex_spaced(bytes)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fmt::Endian;

    #[test]
    fn guess_empty() {
        let (label, text) = guess(&[], Endian::Little);
        let t = crate::i18n::tr();
        assert_eq!(label, t.guess_empty_label);
        assert_eq!(text, t.guess_empty_sym);
    }

    #[test]
    fn guess_utf8_printable() {
        let (label, text) = guess(b"hello", Endian::Little);
        let t = crate::i18n::tr();
        assert_eq!(label, t.guess_utf8);
        assert_eq!(text, "hello");
    }

    #[test]
    fn guess_utf8_with_whitespace() {
        // \t \n \r 也算可打印
        let (label, _) = guess(b"a\tb\nc\r", Endian::Little);
        let t = crate::i18n::tr();
        assert_eq!(label, t.guess_utf8);
    }

    #[test]
    fn guess_utf8_control_rejected() {
        // 含 \0 控制字符，不走 UTF-8
        let (label, _) = guess(b"a\x00b", Endian::Little);
        let t = crate::i18n::tr();
        assert_ne!(label, t.guess_utf8);
    }

    #[test]
    fn guess_u16_le() {
        let bytes = [0x34, 0x12]; // LE 0x1234 = 4660
        let (label, text) = guess(&bytes, Endian::Little);
        assert!(label.contains("u16"));
        assert!(label.contains("LE"));
        // 4660 是 4 位数，不加千位分隔
        assert_eq!(text, "4660");
    }

    #[test]
    fn guess_u32_le() {
        let bytes = [0x78, 0x56, 0x34, 0x12]; // LE 0x12345678
        let (label, text) = guess(&bytes, Endian::Little);
        assert!(label.contains("u32"));
        assert!(label.contains("LE"));
        assert_eq!(text, "305,419,896");
    }

    #[test]
    fn guess_u64_be() {
        let bytes = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2A]; // BE 42
        let (label, text) = guess(&bytes, Endian::Big);
        assert!(label.contains("u64"));
        assert!(label.contains("BE"));
        assert_eq!(text, "42");
    }

    #[test]
    fn guess_binary_for_other_lengths() {
        let bytes = [0xAB, 0xCD, 0xEF]; // 3 字节：非文本、非 2/4/8
        let (label, text) = guess(&bytes, Endian::Little);
        let t = crate::i18n::tr();
        assert_eq!(label, t.guess_binary);
        assert_eq!(text, "AB CD EF");
    }

    #[test]
    fn guess_single_byte_binary() {
        let bytes = [0x80]; // 非法 UTF-8，1 字节走 binary 分支
        let (label, text) = guess(&bytes, Endian::Little);
        let t = crate::i18n::tr();
        assert_eq!(label, t.guess_binary);
        assert_eq!(text, "80");
    }
}
