// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 一张 Key/Value 卡片的编排：标题行 → 导航行 → 文本区 → hex 区。

mod header;
mod hex;
mod nav;
mod text;

use super::layout;
use super::state::Side;
use crate::fmt::{self, DecodeMode};
use crate::ui::MdbxerApp;

/// 一张卡片本帧的渲染上下文（各分区共享，避免重复取值与重复估算）。
pub(super) struct CardCtx<'a> {
    /// 卡片角色
    pub side: Side,
    /// 卡片完整字节
    pub bytes: &'a [u8],
    /// 当前段窗口
    pub window: &'a [u8],
    /// 当前段起始偏移
    pub off: usize,
    /// 当前段已解码文本
    pub text: &'a str,
    /// 本帧解码格式（标题行交互之后取一次）
    pub mode: DecodeMode,
    /// 卡片可用高度
    pub budget: f32,
    /// 固定开销高度（标题/导航/两个折叠头）
    pub fixed: f32,
    /// 文本区是否展开
    pub text_open: bool,
    /// hex 区是否展开
    pub hex_open: bool,
}

/// 渲染一张卡片，返回其实际高度。
///
/// `side` 决定角色；`bytes` 为卡片完整字节，`key` 为选中行的 Key 字节
/// （多值导航操作要用）；`text` 为当前段窗口已解码文本；`budget` 为可用高度上限。
#[allow(clippy::too_many_arguments)]
pub(super) fn show(
    ui: &mut egui::Ui,
    app: &mut MdbxerApp,
    title: &str,
    bytes: &[u8],
    side: Side,
    key: &[u8],
    text: &str,
    budget: f32,
) -> f32 {
    let dup_sort = app.cur_table().map(|tbl| tbl.dup_sort).unwrap_or(false);
    let save_base = save_base_name(app, side, dup_sort);

    egui::Frame::group(ui.style())
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // 卡片内容起始游标：hex 区高度用"预算 − 已用"实测回填，
            // 不靠前文估算，保证卡片始终撑满分到的预算
            let card_top = ui.cursor().min.y;

            header::title_row(ui, app, side, title, bytes, &save_base);
            let mode = app.detail.mode_of(side);
            if mode == DecodeMode::Proto {
                header::proto_row(ui, app, side);
            }

            let total = bytes.len();
            let (off, window) = layout::card_window(app, bytes, side);
            if total > fmt::PAGE_BYTES {
                nav::seg_row(ui, app, side, total, off);
            }
            if total > 512 {
                nav::blob_search_row(ui, app, side, bytes);
            }
            let is_dup = !side.is_key() && dup_sort;
            if is_dup {
                nav::dup_rows(ui, app, key);
            }

            let fixed = layout::card_fixed_overhead(total, is_dup);
            let (text_open, hex_open) = layout::card_section_open(ui.ctx(), side);
            let ctx = CardCtx {
                side,
                bytes,
                window,
                off,
                text,
                mode,
                budget,
                fixed,
                text_open,
                hex_open,
            };
            text::show(ui, app, &ctx);
            hex::show(ui, app, &ctx, card_top);
        })
        .response
        .rect
        .height()
}

/// 另存文件名前缀：Key / Value（多值时带序号）。
fn save_base_name(app: &MdbxerApp, side: Side, dup_sort: bool) -> String {
    if side.is_key() {
        "key".to_string()
    } else if dup_sort {
        format!("value_{:06}", app.detail.dup.index + 1)
    } else {
        "value".to_string()
    }
}
