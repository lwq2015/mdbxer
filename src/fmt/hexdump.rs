//! HEX 转储（可开关 地址/HEX/ASCII 三段，行宽可选）。

const MAX_DUMP: usize = 64 * 1024;

/// 可选行宽（每行字节数）。
pub const HEX_WIDTHS: [usize; 4] = [4, 8, 16, 32];
pub const DEFAULT_HEX_WIDTH: usize = 8;

/// 按 `width` 字节一行的 hex dump。三个开关分别控制 地址列 / 十六进制列 / ASCII 列。
pub fn hex_dump(
    bytes: &[u8],
    width: usize,
    show_addr: bool,
    show_hex: bool,
    show_ascii: bool,
) -> String {
    if bytes.is_empty() {
        return "（空）".to_string();
    }
    let width = width.max(1);
    let truncated = bytes.len() > MAX_DUMP;
    let bytes = &bytes[..bytes.len().min(MAX_DUMP)];

    // 宽行在中间加一道额外空隙便于阅读
    let mid_gap = if width >= 8 { width / 2 } else { usize::MAX };

    let mut out = String::new();
    for (line, chunk) in bytes.chunks(width).enumerate() {
        let mut s = String::new();
        if show_addr {
            s.push_str(&format!("{:08X}  ", line * width));
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
    if truncated {
        out.push_str(&format!("…（仅显示前 {MAX_DUMP} 字节）"));
    }
    out
}
