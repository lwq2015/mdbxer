// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 卡片布局估算：段窗口、折叠状态、自然高度/预算分配、右栏宽度。

use super::state::{DetailState, Side};
use crate::fmt;
use crate::ui::MdbxerApp;

/// 卡片当前段窗口（偏移自动夹到有效范围），供高度估算与渲染共用。
pub(super) fn card_window<'b>(app: &MdbxerApp, bytes: &'b [u8], side: Side) -> (usize, &'b [u8]) {
    let total = bytes.len();
    let off = app.detail.seg_off(side);
    let off = if off >= total {
        total.saturating_sub(fmt::PAGE_BYTES.min(total))
    } else {
        off
    };
    let end = (off + fmt::PAGE_BYTES).min(total);
    (off, &bytes[off..end])
}

/// 卡片文本/hex 两区的折叠状态。估算与渲染必须使用同一组 Id，
/// 这里用全局 `Id::new`（不用 `ui.make_persistent_id`，否则会带上
/// ScrollArea/分组的路径前缀，show() 里预估算时对不上）。
pub(super) fn card_section_open(ctx: &egui::Context, side: Side) -> (bool, bool) {
    // 文本区默认展开：卡片高度已自适应（内部 ScrollArea + 16 行上限），
    // 大值折叠反而要多点一次；Id 盐值随之固定（不再随大小改变默认值）
    let text_open = egui::collapsing_header::CollapsingState::load_with_default_open(
        ctx,
        egui::Id::new(("detail_text", side.is_key(), true)),
        true,
    )
    .is_open();
    let hex_open = egui::collapsing_header::CollapsingState::load_with_default_open(
        ctx,
        egui::Id::new(("detail_hex", side.is_key())),
        true,
    )
    .is_open();
    (text_open, hex_open)
}

/// 卡片自然高度估算：固定开销（标题/导航/搜索/两个折叠头/间距）
/// + 文本区（≤16 行）+ hex 区（行数 × 行高）。
///
/// 折叠的区按 0 计，高度让给其它区（折叠一方后可用高度理应更多）。
pub(super) fn card_natural_height(
    ui: &egui::Ui,
    app: &MdbxerApp,
    total: usize,
    window: &[u8],
    text: &str,
    side: Side,
    dup_nav: bool,
) -> f32 {
    // 行高与 Hex 视图一致（不加 extra_text_line_spacing）：
    // 保证文本区预估高度与实际渲染（fold_view_body / TextEdit）完全吻合。
    let line_h = ui.fonts_mut(|f| f.row_height(&egui::TextStyle::Monospace.resolve(ui.style())));
    let fixed = card_fixed_overhead(total, dup_nav);
    let (text_open, hex_open) = card_section_open(ui.ctx(), side);
    let budget = HeightBudget {
        line_h,
        content_budget: f32::INFINITY,
        text_open,
        hex_open,
    };
    let number_text_display_rows = number_text_display_rows(app, side, window);
    let (text_h, hex_h) =
        card_content_heights(ui, app, window, text, &budget, number_text_display_rows);
    fixed + text_h + hex_h
}

pub(super) fn number_text_display_rows(app: &MdbxerApp, side: Side, bytes: &[u8]) -> bool {
    let mode = app.detail.mode_of(side);
    mode == fmt::DecodeMode::Hex
        || (mode == fmt::DecodeMode::Auto
            && fmt::guess(bytes, app.endian, &app.hint_value).0 == crate::i18n::tr().guess_binary)
}

/// 卡片固定开销：标题行 + 两个折叠标题行 + 可选的分段/搜索/多值导航行 + 间距。
pub(super) fn card_fixed_overhead(total: usize, dup_nav: bool) -> f32 {
    let mut h = 26.0 + 24.0 * 2.0 + 20.0; // 标题 + 文本头 + hex 头 + 间距
    if total > fmt::PAGE_BYTES {
        h += 28.0; // 分段导航行
    }
    if total > 512 {
        h += 28.0; // 字段内搜索行
    }
    if dup_nav {
        h += 56.0; // 多值导航两行
    }
    h
}

/// 文本/hex 内容区高度分配参数。
pub(super) struct HeightBudget {
    /// 等宽字体行高（与 hex 视图一致）
    pub line_h: f32,
    /// 内容区可用高度；`f32::INFINITY` 表示只估算自然高度
    pub content_budget: f32,
    /// 文本区是否展开
    pub text_open: bool,
    /// hex 区是否展开
    pub hex_open: bool,
}

/// 文本/hex 内容区在预算下的实际高度分配：
/// 都装得下按自然高度；装不下时至少各分一半，小的一方把余量让给大的一方；
/// 折叠的区分 0、预算全部给展开的一方。
pub(super) fn card_content_heights(
    ui: &egui::Ui,
    app: &MdbxerApp,
    window: &[u8],
    text: &str,
    budget: &HeightBudget,
    number_text_display_rows: bool,
) -> (f32, f32) {
    let HeightBudget {
        line_h,
        content_budget,
        text_open,
        hex_open,
    } = *budget;
    // 文本自然高度按真实折行计算（与 TextEdit 渲染一致，防止长逻辑行被估成 1 行）；
    // galley 由 egui 全局缓存，重复估算开销可忽略。
    // 16 行封顶只用于"无限预算的自然高度估算"，避免大文本把卡片估成无限高；
    // 有限预算（实际渲染）用真实高度参与下方分配，这样 hex 折叠、预算释放时，
    // 文本区才能和 hex 区一样占满整块空间（否则会永久停在 16 行、下方留白）。
    let text_nat = if text_open {
        let font_id = egui::TextStyle::Monospace.resolve(ui.style());
        let line_number_gutter = if app.detail.show_text_line_numbers {
            let font_id = egui::TextStyle::Monospace.resolve(ui.style());
            let digit_width = ui.fonts_mut(|fonts| fonts.glyph_width(&font_id, '0'));
            let digits = if number_text_display_rows {
                let available_width = (ui.available_width() - 40.0).max(digit_width);
                let chars = text.chars().count();
                let mut digits = 1;
                loop {
                    let text_width =
                        (available_width - digits as f32 * digit_width - 8.0).max(digit_width);
                    let chars_per_row = (text_width / digit_width).floor().max(1.0) as usize;
                    let rows = chars.div_ceil(chars_per_row).max(1);
                    let next_digits = rows.ilog10() + 1;
                    if next_digits == digits {
                        break digits;
                    }
                    digits = next_digits;
                }
            } else {
                let line_count = text.bytes().filter(|&b| b == b'\n').count() + 1;
                line_count.ilog10() + 1
            };
            digits as f32 * digit_width + 8.0
        } else {
            0.0
        };
        let wrap_w = (ui.available_width() - 40.0 - line_number_gutter).max(80.0);
        let galley =
            ui.fonts_mut(|f| f.layout(text.to_string(), font_id, egui::Color32::WHITE, wrap_w));
        let raw_h = galley.size().y.max(line_h);
        let h = if content_budget.is_finite() {
            raw_h
        } else {
            raw_h.min(16.0 * line_h)
        };
        h + 12.0
    } else {
        0.0
    };
    let hex_rows = window.len().div_ceil(app.detail.hex_width).max(1);
    let hex_nat = if hex_open {
        hex_rows as f32 * line_h + 12.0
    } else {
        0.0
    };
    if text_nat + hex_nat <= content_budget {
        // 文本是主阅读区：预算充足时把剩余全部给文本区，使 text_h 足够大，
        // 确保内容能完整显示、不触发滚动条（下方留白由内容本身决定）。
        // auto_shrink 保持收缩：内容不足 text_h 时区域收缩到内容高，不留空白滚动区。
        if text_open {
            return (content_budget - hex_nat, hex_nat);
        }
        return (text_nat, content_budget - text_nat);
    }
    let half = content_budget / 2.0;
    if text_nat <= half {
        (text_nat, content_budget - text_nat)
    } else if hex_nat <= half {
        (content_budget - hex_nat, hex_nat)
    } else {
        (half, half)
    }
}

/// 当前 hex 配置（行宽 + 地址/HEX/ASCII 开关）下内容实际所需宽度。
/// `addr_max_off` 为当前数据（Key/Value 中较大者）的最大绝对偏移，决定地址列位数。
pub(super) fn detail_needed_width(ui: &egui::Ui, d: &DetailState, addr_max_off: usize) -> f32 {
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font_id, '0'));

    let n = d.hex_width as f32;
    let mut line_chars = 0.0_f32;
    if d.show_hex_line_numbers {
        let rows = (addr_max_off + 1).div_ceil(d.hex_width).max(1);
        line_chars += (rows.ilog10() + 2) as f32;
    }
    if d.show_addr {
        // 地址位数随数据大小变化（最小 4 位）+ 列后 2 空格
        line_chars += fmt::addr_chars(fmt::addr_digits(0, addr_max_off + 1)) as f32;
    }
    if d.show_hex {
        // 每字节 2~3 字符（紧凑 2 / 常规 3），宽行中间额外 1 个分隔空格
        line_chars += fmt::hex_section_chars(d.hex_width, d.hex_compact) as f32;
    }
    if d.show_ascii {
        line_chars += n;
    }
    // 固定开销按实际布局算，别用拍脑袋的常数：估大了右栏最小宽度过宽、
    // 内容右侧留一条明显空白；估小了内容超宽会被裁。
    // - 面板 frame（side_top_panel）：内边距 8 + 8，可拖边留 1px 分隔线；
    // - 卡片 group frame：内边距 6 + 6，边框 1×2（egui 把 stroke 计入总边距）；
    // - 折叠体缩进：spacing.indent（只在左侧）；
    // - 浮动滚动条不占宽度但会压在内容右缘，按其宽度预留，免得压住 ASCII 末字符；
    // - 末尾 2px 取整余量。
    let chrome = 8.0
        + 8.0
        + 1.0
        + 6.0
        + 6.0
        + 2.0
        + ui.spacing().indent
        + ui.spacing().scroll.bar_width
        + 2.0;
    line_chars * char_w + chrome
}

#[cfg(test)]
mod tests {
    use super::card_fixed_overhead;
    use crate::fmt;

    #[test]
    fn fixed_overhead_base_has_no_extra_rows() {
        assert_eq!(card_fixed_overhead(100, false), 94.0);
    }

    #[test]
    fn fixed_overhead_adds_segment_row_only() {
        // 超过一个段：+28（段导航）；同时内容 >512 仍带搜索行
        assert_eq!(
            card_fixed_overhead(fmt::PAGE_BYTES + 1, false),
            94.0 + 28.0 + 28.0
        );
        // 恰好一个段：没有段导航，但仍 >512 有搜索行
        assert_eq!(card_fixed_overhead(fmt::PAGE_BYTES, false), 94.0 + 28.0);
    }

    #[test]
    fn fixed_overhead_adds_search_and_dup_rows() {
        // >512：搜索行 +28；多值：导航两行 +56
        assert_eq!(card_fixed_overhead(513, false), 94.0 + 28.0);
        assert_eq!(card_fixed_overhead(513, true), 94.0 + 28.0 + 56.0);
    }
}
