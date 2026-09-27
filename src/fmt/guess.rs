//! 字节内容的自动类型猜测。

use super::value;

/// 猜测字节内容的类型，返回 (类型标签, 显示文本)。
pub fn guess(bytes: &[u8]) -> (&'static str, String) {
    if bytes.is_empty() {
        return ("空", "∅ 空".to_string());
    }
    // 优先可打印 UTF-8 文本（允许 \t \n \r）
    if let Ok(s) = std::str::from_utf8(bytes) {
        if s.chars()
            .all(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r'))
        {
            return ("UTF-8 文本", s.to_string());
        }
    }
    match bytes.len() {
        8 => (
            "u64 LE",
            value::u64_text(u64::from_le_bytes(bytes.try_into().unwrap())),
        ),
        4 => (
            "u32 LE",
            u32::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        ),
        2 => (
            "u16 LE",
            u16::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        ),
        _ => ("二进制", value::hex_spaced(bytes)),
    }
}
