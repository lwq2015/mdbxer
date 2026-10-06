// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! HEX 转储（可开关 地址/HEX/ASCII 三段，行宽可选）。

/// 详情区 hex/文本视图每段渲染的字节数（64 KiB）。
/// 超出此长度的数据按偏移分段查看，避免一次性渲染超大文本卡界面。
pub const PAGE_BYTES: usize = 64 * 1024;

/// 可选行宽（每行字节数）。
pub const HEX_WIDTHS: [usize; 4] = [4, 8, 16, 32];
pub const DEFAULT_HEX_WIDTH: usize = 8;

/// 地址列宽度（字符）：8 位十六进制 + 2 空格。
pub const ADDR_CHARS: usize = 10;

/// 宽行（≥8 字节）中间加空隙的位置（第 mid 字节前多 1 空格）。
pub fn mid_gap_index(width: usize) -> Option<usize> {
    (width >= 8).then_some(width / 2)
}

/// HEX 列总宽（字符）：每字节 "XX " 占 3 字符 + 宽行中间 1 个空隙。
pub fn hex_section_chars(width: usize) -> usize {
    width * 3 + if mid_gap_index(width).is_some() { 1 } else { 0 }
}

/// 构造一行 hex dump（不去尾空格，供交互组件按固定字符坐标布局）。
/// 与 [`hex_dump`] 的单行内容一致：地址列 + HEX 列（末行补空格对齐）+ ASCII 列。
pub fn hex_line(
    chunk: &[u8],
    width: usize,
    show_addr: bool,
    show_hex: bool,
    show_ascii: bool,
    abs_offset: usize,
) -> String {
    let width = width.max(1);
    let mid_gap = mid_gap_index(width).unwrap_or(usize::MAX);
    let mut s = String::new();
    if show_addr {
        s.push_str(&format!("{abs_offset:08X}  "));
    }
    if show_hex {
        for i in 0..width {
            if i == mid_gap {
                s.push(' ');
            }
            match chunk.get(i) {
                Some(b) => s.push_str(&format!("{b:02X} ")),
                None => s.push_str("   "),
            }
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
    s
}

/// 生成 HEX 列复制文本：与屏幕显示列对齐。
///
/// 选中区间 `lo..=hi`（全局字节序号，端点自动排序/夹取）跨显示行时按行换行，
/// 每行从该行完整 HEX 段中切出被选中的字符区间——行首保留空格缩进，
/// 使粘贴到等宽编辑器后各字节仍落在与视图相同的列；宽行 mid gap 同样保留。
/// 行尾不保留尾随空格。
pub fn hex_copy_selection(bytes: &[u8], lo: usize, hi: usize, width: usize) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let n = width.max(1);
    let (lo, hi) = (lo.min(hi), lo.max(hi).min(bytes.len() - 1));
    if lo > hi {
        return String::new();
    }
    let mid = mid_gap_index(n).unwrap_or(usize::MAX);
    // 行内第 i 字节在纯 HEX 段文本中的起始字符下标
    let cstart = |i: usize| 3 * i + if i >= mid { 1 } else { 0 };

    let mut lines = Vec::new();
    let mut r = lo / n;
    let last_row = hi / n;
    while r <= last_row {
        let row_start = r * n;
        let row_end = (row_start + n).min(bytes.len());
        let sel_lo = lo.max(row_start) - row_start;
        let sel_hi = hi.min(row_end - 1) - row_start; // 行内最后一个选中字节
        let line = hex_line(&bytes[row_start..row_end], n, false, true, false, 0);
        let sc = cstart(sel_lo);
        let ec = cstart(sel_hi) + 2; // 不含该字节尾随的空格
        // 行首未选中的字节用等宽空格顶替（保留列对齐，含 mid gap 宽度）
        let mut out = String::with_capacity(ec);
        out.push_str(&" ".repeat(sc));
        out.push_str(&line[sc..ec]);
        lines.push(out);
        r += 1;
    }
    lines.join("\n")
}

/// 按 `width` 字节一行的 hex dump。三个开关分别控制 地址列 / 十六进制列 / ASCII 列。
///
/// `base_offset` 为这段数据在原始字节序列中的起始偏移，
/// 地址列显示的是绝对偏移（分段查看时每段从上一段末尾继续编号）。
///
/// 空数据返回"（空）"。
///
/// 当前界面渲染走自绘的 `ui::hexview`，本函数保留为工具函数与测试基准。
#[allow(dead_code)]
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
    let mut out = String::new();
    for (line, chunk) in bytes.chunks(width).enumerate() {
        out.push_str(&hex_line(
            chunk,
            width,
            show_addr,
            show_hex,
            show_ascii,
            base_offset + line * width,
        ));
        out.truncate(out.trim_end().len());
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

    #[test]
    fn hex_line_is_untrimmed_and_ascii_aligned() {
        // 行宽 4、只 1 字节：HEX 列补齐 4 字节（12 字符），ASCII 起始位置固定
        let line = hex_line(&[0x45], 4, false, true, true, 0);
        assert_eq!(line.len(), 13); // 12（HEX 列）+ 1（ASCII）
        assert_eq!(&line[..2], "45");
        assert_eq!(line.chars().last(), Some('E'));
        assert!(line[2..12].chars().all(|c| c == ' '));
        // 几何常量与之一致
        assert_eq!(hex_section_chars(4), 12);
        assert_eq!(hex_section_chars(8), 25); // 8*3 + 中间空隙 1
        assert_eq!(mid_gap_index(8), Some(4));
        assert_eq!(mid_gap_index(4), None);
    }

    #[test]
    fn hex_line_mid_gap_offset() {
        // 行宽 8：第 5 字节（i=4=mid）前多 1 空格
        let line = hex_line(&[0u8; 8], 8, false, true, false, 0);
        // "00 00 00 00  00 00 00 00 "（结尾空格保留）
        assert!(line.starts_with("00 00 00 00  00"));
        assert_eq!(line.len(), hex_section_chars(8));
    }

    #[test]
    fn hex_copy_selection_aligns_columns() {
        // "Hello, MDBX!" 12 字节，行宽 8
        let b = b"Hello, MDBX!";

        // 行内单行部分选择：字节 1..=3 → 3 空格缩进（对齐第 2 字节列）
        assert_eq!(hex_copy_selection(b, 1, 3, 8), "   65 6C 6C");
        // 从行首选则无缩进
        assert_eq!(hex_copy_selection(b, 0, 3, 8), "48 65 6C 6C");

        // 跨两行且首行从字节 6 起：第一行缩进 3*6+mid 空隙 1 = 19 空格
        let s = hex_copy_selection(b, 6, 11, 8);
        assert_eq!(s, "                   20 4D\n44 42 58 21");
        // 第二行 4 字节顶格（它在显示中就是行首）
        assert!(s.ends_with("\n44 42 58 21"));

        // 端点反序也归一化
        assert_eq!(hex_copy_selection(b, 3, 1, 8), "   65 6C 6C");
        // 超界夹取
        assert_eq!(hex_copy_selection(b, 0, 99, 8).lines().count(), 2);
    }

    #[test]
    fn hex_copy_selection_mid_gap_indent() {
        let b = (0u8..16).collect::<Vec<u8>>();
        // 选第二行字节 4..7：缩进 = 3*4 + mid 空隙 1 = 13 空格
        let s = hex_copy_selection(&b, 12, 15, 8);
        assert_eq!(s, "             0C 0D 0E 0F");
        assert_eq!(s.find('0'), Some(13));

        // 行内选中跨越 mid gap（字节 2..5）：选区内保留双空格间隙
        let s = hex_copy_selection(&b, 2, 5, 8);
        assert_eq!(s, "      02 03  04 05");

        // 行宽 4（无 mid gap）：缩进 = 2*3 = 6 空格
        let s = hex_copy_selection(b"ABCD", 2, 3, 4);
        assert_eq!(s, "      43 44");
    }
}
