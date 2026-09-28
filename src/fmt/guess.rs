//! 字节内容的自动类型猜测。

use super::value;
use super::Endian;

/// 猜测字节内容的类型，返回 (类型标签, 显示文本)。
/// 多字节整数按 endian 解释。
pub fn guess(bytes: &[u8], endian: Endian) -> (String, String) {
    if bytes.is_empty() {
        return ("空".to_string(), "∅ 空".to_string());
    }
    // 优先可打印 UTF-8 文本（允许 \t \n \r）
    if let Ok(s) = std::str::from_utf8(bytes) {
        if s.chars()
            .all(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r'))
        {
            return ("UTF-8 文本".to_string(), s.to_string());
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
            (format!("u32 {sfx}"), v.to_string())
        }
        2 => {
            let arr: [u8; 2] = bytes.try_into().unwrap_or_else(|_| unreachable!());
            let v = match endian {
                Endian::Little => u16::from_le_bytes(arr),
                Endian::Big => u16::from_be_bytes(arr),
            };
            (format!("u16 {sfx}"), v.to_string())
        }
        _ => ("二进制".to_string(), value::hex_spaced(bytes)),
    }
}
