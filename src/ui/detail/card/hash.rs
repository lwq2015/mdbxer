// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 卡片底部"校验"行：显示一种校验和（MD5 / SHA-1 / SHA-256），点击即复制。
//! 对卡片完整原始字节计算，与解码格式、单元格截断和分段无关。
//!
//! 高度先由预排版实测得到，再交给 hex 区从预算里扣除，
//! 因此卡片总高度始终贴合分到的预算，不会把右栏撑出滚动条。

use super::super::state::Side;
use crate::i18n::tr;
use crate::ui::{MdbxerApp, Status};

/// 分隔线与上下间距占位（分隔线 1px + 前后各一次 item_spacing）。
const ROW_EXTRA: f32 = 18.0;

/// 预排好的一行校验和：绘制时复用同一份布局，保证实测高度与绘制结果一致。
pub(super) struct HashRow {
    job: egui::text::LayoutJob,
    hex: String,
    alg: &'static str,
    /// 本行占位高度（文本高度 + 分隔线/间距）
    pub height: f32,
}

/// 取缓存或计算校验和，并按当前可用宽度预排版。
/// 返回 None 表示当前配置为"不显示"。
pub(super) fn prepare(
    ui: &egui::Ui,
    app: &mut MdbxerApp,
    side: Side,
    bytes: &[u8],
) -> Option<HashRow> {
    let alg = app.hash_alg?;
    // 缓存键：换表/换行/换多值/字节长度或算法变化才重算
    let key = (
        app.selected_table,
        app.selected_row.unwrap_or(0),
        app.detail.dup.index,
        bytes.len(),
    );
    let card = app.detail.card_mut(side);
    let stale = match &card.hash_cache {
        Some((cached_key, cached_alg, _)) => *cached_key != key || *cached_alg != alg,
        None => true,
    };
    if stale {
        card.hash_cache = Some((key, alg, alg.hex(bytes)));
    }
    let (_, _, hex) = card.hash_cache.as_ref().expect("刚写入的校验和缓存");
    let hex = hex.clone();

    // 与绘制时同一份排版：算法名与十六进制等宽小字
    let small = egui::TextStyle::Small.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = ui.available_width();
    let hash_fmt = egui::TextFormat {
        font_id: egui::FontId::monospace(small.size),
        color: ui.visuals().text_color(),
        ..Default::default()
    };
    job.append(&format!("{}: {hex}", alg.name()), 0.0, hash_fmt);

    let text_h = ui.fonts_mut(|f| f.layout_job(job.clone())).size().y;
    Some(HashRow {
        job,
        hex,
        alg: alg.name(),
        height: text_h + ROW_EXTRA,
    })
}

/// 绘制校验行（位置在卡片最底部）。
pub(super) fn draw(ui: &mut egui::Ui, app: &mut MdbxerApp, row: HashRow) {
    ui.separator();
    let resp = ui
        .add(egui::Label::new(row.job).sense(egui::Sense::click()))
        .on_hover_text(tr().hash_copy_tip);
    if resp.clicked() {
        ui.ctx().copy_text(row.hex.clone());
        app.status = Status::info(tr().hash_copied(row.alg));
    }
}
