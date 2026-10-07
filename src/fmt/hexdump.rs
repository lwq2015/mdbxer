// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! HEX 转储（可开关 地址/HEX/ASCII 三段，行宽可选）。

/// 详情区 hex/文本视图每段渲染的字节数（64 KiB）。
/// 超出此长度的数据按偏移分段查看，避免一次性渲染超大文本卡界面。
pub const PAGE_BYTES: usize = 64 * 1024;

/// 可选行宽（每行字节数）。
pub const HEX_WIDTHS: [usize; 3] = [4, 8, 16 /*, 32*/];
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

/// 生成选区复制文本：每行包含 HEX 段与/或 ASCII 段（跟随界面开关），布局与屏幕一致。
///
/// 选中区间 `lo..=hi`（全局字节序号，端点自动排序/夹取）跨显示行时按行换行；
/// 每行以 [`hex_line`]（关地址列）为底，未选中字节占据的字符位置一律替换为空格——
/// HEX 侧保留列缩进与 mid gap，ASCII 侧未选字符同样空白，行尾尾随空格去除。
/// 两段都开时效果（n=8，选中第二行全部 4 字节）：
/// ```text
/// 44 42 58 21              DBX!
/// ```
pub fn hex_copy_selection(
    bytes: &[u8],
    lo: usize,
    hi: usize,
    width: usize,
    show_hex: bool,
    show_ascii: bool,
) -> String {
    if bytes.is_empty() || (!show_hex && !show_ascii) {
        return String::new();
    }
    let n = width.max(1);
    let (lo, hi) = (lo.min(hi), lo.max(hi).min(bytes.len() - 1));
    if lo > hi {
        return String::new();
    }
    let mid = mid_gap_index(n).unwrap_or(usize::MAX);
    // 行内第 i 字节在 HEX 段中的起始字符下标
    let cstart = |i: usize| 3 * i + if i >= mid { 1 } else { 0 };
    // 只显示 ASCII 时行首即 ASCII 段
    let ascii_base = if show_hex { hex_section_chars(n) } else { 0 };

    let mut lines = Vec::new();
    let mut r = lo / n;
    let last_row = hi / n;
    while r <= last_row {
        let row_start = r * n;
        let row_end = (row_start + n).min(bytes.len());
        let sel_lo = lo.max(row_start) - row_start;
        let sel_hi = hi.min(row_end - 1) - row_start; // 行内最后一个选中字节
        let line = hex_line(
            &bytes[row_start..row_end],
            n,
            false,
            show_hex,
            show_ascii,
            0,
        );
        let mut buf: Vec<char> = line.chars().collect();
        // 只保留选中字节在 HEX 段（两位数字）和 ASCII 段（一个字符）的位置
        let mut keep = vec![false; buf.len()];
        for i in sel_lo..=sel_hi {
            if show_hex {
                for c in cstart(i)..cstart(i) + 2 {
                    keep[c] = true;
                }
            }
            if show_ascii {
                keep[ascii_base + i] = true;
            }
        }
        for (ch, k) in buf.iter_mut().zip(&keep) {
            if !*k {
                *ch = ' ';
            }
        }
        lines.push(buf.iter().collect::<String>().trim_end().to_string());
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
    fn hex_copy_selection_has_hex_and_ascii() {
        // "Hello, MDBX!" 12 字节，行宽 8
        let b = b"Hello, MDBX!";

        // 单行整行选：HEX 段（含 mid gap 双空格）与 ASCII 原文同行
        let s = hex_copy_selection(b, 0, 7, 8, true, true);
        assert_eq!(s, "48 65 6C 6C  6F 2C 20 4D Hello, M");

        // 行内部分选 i1..=3：HEX 带 3 空格缩进，ASCII 段只留 "ell"
        let s = hex_copy_selection(b, 1, 3, 8, true, true);
        assert!(s.starts_with("   65 6C 6C"));
        assert!(s.ends_with("ell"));
        // HEX 末尾到 ASCII 之间全部是空格
        let mid = &s[11..s.len() - 3];
        assert!(mid.bytes().all(|c| c == b' '));

        // 只选第二行全部 4 字节：HEX 顶格 + 补齐空格 + ASCII
        assert_eq!(
            hex_copy_selection(b, 8, 11, 8, true, true),
            "44 42 58 21              DBX!"
        );

        // 跨两行
        let s = hex_copy_selection(b, 6, 11, 8, true, true);
        assert_eq!(s.lines().count(), 2);
        let l1 = s.lines().next().unwrap();
        assert!(l1.starts_with("                   20 4D")); // 19 空格缩进
        assert!(l1.ends_with('M'));
        assert!(s.ends_with("DBX!"));

        // 端点反序归一化、超界夹取
        assert!(hex_copy_selection(b, 3, 1, 8, true, true).ends_with("ell"));
        assert_eq!(
            hex_copy_selection(b, 0, 99, 8, true, true).lines().count(),
            2
        );
    }

    #[test]
    fn hex_copy_selection_mid_gap_and_narrow() {
        let b = (0u8..16).collect::<Vec<u8>>();

        // 选第二行字节 4..7（值 0C..0F，ASCII 都是 '.'）
        let s = hex_copy_selection(&b, 12, 15, 8, true, true);
        assert!(s.starts_with("             0C 0D 0E 0F")); // 13 空格缩进
        assert_eq!(s.find('0'), Some(13));
        assert!(s.ends_with("....")); // 仅 i4..7 四个 ASCII 位保留

        // 行内选中跨越 mid gap（字节 2..5）：HEX 选区内保留双空格
        let s = hex_copy_selection(&b, 2, 5, 8, true, true);
        assert!(s.starts_with("      02 03  04 05"));
        assert!(s.ends_with("..")); // ASCII 侧 i4..5 两个 '.'

        // 行宽 4（无 mid gap）：缩进 6 空格，HEX 与 ASCII 同时出现
        let s = hex_copy_selection(b"ABCD", 2, 3, 4, true, true);
        assert!(s.starts_with("      43 44"));
        assert!(s.ends_with("CD"));
    }

    #[test]
    fn hex_copy_selection_respects_section_toggles() {
        let b = b"Hello, MDBX!";

        // 只显示 ASCII：选第二行全部 → "DBX!"；部分选 i=9..10 → " BX"（行首未选留空格对齐）
        assert_eq!(hex_copy_selection(b, 8, 11, 8, false, true), "DBX!");
        assert_eq!(hex_copy_selection(b, 9, 10, 8, false, true), " BX");

        // 只显示 HEX：第二行全选，尾随补齐空格被 trim
        assert_eq!(hex_copy_selection(b, 8, 11, 8, true, false), "44 42 58 21");

        // 两个开关都关：空串
        assert_eq!(hex_copy_selection(b, 8, 11, 8, false, false), "");
    }
}
