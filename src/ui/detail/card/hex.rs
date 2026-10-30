// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 卡片十六进制区：独立滚动区 + 选区指纹失效 + 命中高亮/滚动。

use super::CardCtx;
use crate::i18n::tr;
use crate::ui::MdbxerApp;
use crate::ui::detail::state::Side;
use crate::ui::hexview;
use crate::ui::syntax::SearchHighlights;

/// 渲染 hex 区（可折叠）。`card_top` 为卡片内容起始游标，用于实测回填高度。
pub(super) fn show(ui: &mut egui::Ui, app: &mut MdbxerApp, ctx: &CardCtx<'_>, card_top: f32) {
    let t = tr();
    let line_h = ui.fonts_mut(|f| f.row_height(&egui::TextStyle::Monospace.resolve(ui.style())));
    // hex 区高度实测回填：吃掉卡片预算减去已用高度（44 ≈ hex 标题行
    // + 分组 frame 下内边距 + 尾部间距），卡片始终撑满、右栏底部不留空白。
    let hex_h = (ctx.budget - (ui.cursor().min.y - card_top) - 44.0).max(line_h * 2.0);

    // 用 CollapsingState + 全局 Id（与 card_section_open 预算估算同一组），
    // 折叠状态变化时高度分配下一帧即自适应。
    egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        egui::Id::new(("detail_hex", ctx.side.is_key())),
        true,
    )
    .show_header(ui, |ui| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(t.section_hex).text_style(egui::TextStyle::Button),
            )
            .selectable(false),
        );
    })
    .body(|ui| {
        // 搜索命中先取（避免与下面 sel 的可变借用冲突）
        let card = match ctx.side {
            Side::Key => &app.detail.key,
            Side::Value => &app.detail.val,
        };
        let active_hit = card.blob_hit;
        let mut highlights = SearchHighlights::default();
        if app.detail.highlight_all_matches
            && let Some(query) = card.blob_query.as_deref()
            && !query.is_empty()
            && query.len() <= ctx.window.len()
        {
            highlights.ranges = super::super::search::find_all_hits(ctx.window, query, ctx.off);
        }
        if let Some(hit) = active_hit
            && !highlights.ranges.contains(&hit)
        {
            highlights.ranges.push(hit);
        }
        highlights.active = active_hit;
        let scroll_to = app.detail.blob_scroll_take(ctx.side);
        // 内容指纹：换行/翻多值/切段后字节变了，选区作废
        let fp = (
            ctx.off,
            ctx.window.len(),
            app.selected_row,
            app.detail.dup.index,
        );

        // hex 显示配置先读成局部量，下面才能可变借用卡片选区
        let width = app.detail.hex_width;
        let show_addr = app.detail.show_addr;
        let show_line_numbers = app.detail.show_hex_line_numbers;
        let show_hex = app.detail.show_hex;
        let show_ascii = app.detail.show_ascii;
        let compact = app.detail.hex_compact;

        let sel_card = match ctx.side {
            Side::Key => &mut app.detail.key,
            Side::Value => &mut app.detail.val,
        };
        let sel_state = &mut sel_card.hex;
        if sel_state.fp != Some(fp) {
            sel_state.sel = None;
        }

        // 独立滚动区：宽度填满、高度随内容收缩；超过卡片分到的预算才出滚动条。
        // 每行字节数固定为工具栏所选（4/8/16）；面板最小宽度按当前配置所需
        // 行宽钳制（见 detail/mod.rs 的 size_range），整行始终完整可见，
        // 不会横向溢出，也不会有滚动条压住 ASCII 列。
        egui::ScrollArea::vertical()
            .id_salt(("detail_hex_scroll", ctx.side.is_key()))
            .auto_shrink([false, true])
            .max_height(hex_h)
            .show(ui, |ui| {
                hexview::hex_view(
                    ui,
                    hexview::view_id(ctx.side.is_key()),
                    ctx.window,
                    width,
                    show_addr,
                    show_line_numbers,
                    show_hex,
                    show_ascii,
                    compact,
                    ctx.off,
                    &mut sel_state.sel,
                    &highlights,
                    scroll_to,
                );
            });
        sel_state.fp = Some(fp);
    });
}
