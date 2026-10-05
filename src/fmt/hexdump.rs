// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! HEX 转储（可开关 地址/HEX/ASCII 三段，行宽可选）。

/// 详情区 hex/文本视图每段渲染的字节数（64 KiB）。
/// 超出此长度的数据按偏移分段查看，避免一次性渲染超大文本卡界面。
pub const PAGE_BYTES: usize = 64 * 1024;

/// 可选行宽（每行字节数）。
pub const HEX_WIDTHS: [usize; 4] = [4, 8, 16, 32];
pub const DEFAULT_HEX_WIDTH: usize = 8;

/// 按 `width` 字节一行的 hex dump。三个开关分别控制 地址列 / 十六进制列 / ASCII 列。
///
/// `base_offset` 为这段数据在原始字节序列中的起始偏移，
/// 地址列显示的是绝对偏移（分段查看时每段从上一段末尾继续编号）。
///
/// 空数据返回"（空）"。
pub fn hex_dump(
    bytes: &[u8],
    width: usize,
    show_addr: bool,
    show_hex: bool,
    show_ascii: bool,
    base_offset: usize,
) -> String {
    if bytes.is_empty() {
        return crate::i18n::tr().empty.to_string();
    }
    let width = width.max(1);

    // 宽行在中间加一道额外空隙便于阅读
    let mid_gap = if width >= 8 { width / 2 } else { usize::MAX };

    let mut out = String::new();
    for (line, chunk) in bytes.chunks(width).enumerate() {
        let mut s = String::new();
        if show_addr {
            s.push_str(&format!("{:08X}  ", base_offset + line * width));
        }
        if show_hex {
            for (i, b) in chunk.iter().enumerate() {
                if i == mid_gap {
                    s.push(' ');
                }
                s.push_str(&format!("{b:02X} "));
            }
            // 末行不足一行时补齐，保证 ASCII 列对齐
            for i in chunk.len()..width {
                if i == mid_gap {
                    s.push(' ');
                }
                s.push_str("   ");
            }
        }
        if show_ascii {
            for &b in chunk {
                s.push(if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                });
            }
        }
        out.push_str(s.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_returns_placeholder() {
        let s = hex_dump(&[], 8, true, true, true, 0);
        assert_eq!(s, crate::i18n::tr().empty);
    }

    #[test]
    fn all_columns_on() {
        let bytes = [0x48, 0x65, 0x6C, 0x6C, 0x6F]; // "Hello"
        let s = hex_dump(&bytes, 8, true, true, true, 0);
        // 地址列
        assert!(s.starts_with("00000000  "));
        // HEX 列（宽 8 时第 4 字节后有双空格）
        assert!(s.contains("48 65 6C 6C  6F"));
        // ASCII 列
        assert!(s.trim_end().ends_with("Hello"));
    }

    #[test]
    fn addr_only() {
        let s = hex_dump(&[0x41], 8, true, false, false, 0);
        assert!(s.starts_with("00000000"));
        assert!(!s.contains("41")); // 无 HEX 列
    }

    #[test]
    fn hex_only_no_addr() {
        let s = hex_dump(&[0x41, 0x42], 8, false, true, false, 0);
        assert!(!s.contains("00000000"));
        assert!(s.contains("41 42"));
    }

    #[test]
    fn ascii_replaces_non_graphic() {
        let s = hex_dump(&[0x41, 0x00, 0x42, 0x07], 8, false, false, true, 0);
        assert!(s.contains("A.B.")); // 0x00 和 0x07 显示为点
    }

    #[test]
    fn width_wraps_lines() {
        let bytes = [0x01, 0x02, 0x03, 0x04, 0x05];
        let s = hex_dump(&bytes, 2, true, true, true, 0);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 3); // 3 行：2+2+1
        assert!(lines[0].contains("01 02"));
        assert!(lines[1].contains("03 04"));
        assert!(lines[2].contains("05"));
    }

    #[test]
    fn base_offset_affects_addr_column() {
        let s = hex_dump(&[0x41], 8, true, true, true, 0x1000);
        assert!(s.starts_with("00001000  "));
    }

    #[test]
    fn mid_gap_for_width_ge_8() {
        // 宽 8 时，中间第 4 字节后多一个空格
        let bytes = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let s = hex_dump(&bytes, 8, false, true, false, 0);
        // "01 02 03 04  05 06 07 08 " 中间双空格
        assert!(s.contains("04  05"), "got: {s}");
    }

    #[test]
    fn no_mid_gap_for_width_lt_8() {
        let bytes = [0x01, 0x02, 0x03, 0x04];
        let s = hex_dump(&bytes, 4, false, true, false, 0);
        assert!(!s.contains("  ")); // 无双空格
    }

    #[test]
    fn last_line_padded_for_ascii_alignment() {
        // 行宽 4，最后一行只有 1 字节，HEX 列应补齐到 4 字节宽
        let bytes = [0x41, 0x42, 0x43, 0x44, 0x45];
        let s = hex_dump(&bytes, 4, true, true, true, 0);
        let last = s.lines().last().unwrap();
        // 末行：地址 + "45" + 补齐空格 + ASCII "E"
        assert!(last.ends_with("E"));
    }
}
