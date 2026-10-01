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
        return "（空）".to_string();
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
