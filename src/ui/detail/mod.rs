// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 右侧详情：Key / Value 卡片（格式下拉、复制、文本、hex dump、多值翻页）。
//!
//! 分层：`state`（纯数据状态）/ `search`、`dup`（纯逻辑）/ `plan`（特殊格式
//! 解析接口）/ `layout`（纯估算）/ `fold`（自绘折叠视图）/ `card`（卡片 UI）。

mod card;
mod dup;
mod export;
mod fold;
mod layout;
mod plan;
mod search;
mod state;

pub use dup::DupCtx;
pub use state::{DetailState, Side};

use crate::fmt;
use crate::i18n::tr;
use crate::ui::MdbxerApp;

/// 两张卡片之间的间距（点）。
const CARD_GAP: f32 = 8.0;

/// 右栏入口：无选中行时显示提示；有选中行时显示 Key/Value 两张卡片。
pub fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let t = tr();
    let left_w = if app.left_visible {
        app.left_panel_w
    } else {
        0.0
    };
    // 地址位数随数据大小变化：取 Key/Value 两卡片中较大的最大绝对偏移
    let val_len = app.current_value().map_or(0, |v| v.len());
    let key_len = app
        .selected_row
        .and_then(|i| app.rows.get(i))
        .map_or(0, |r| r.key.len());
    let addr_max_off = (app.detail.seg_off(Side::Key) + key_len.saturating_sub(1))
        .max(app.detail.seg_off(Side::Value) + val_len.saturating_sub(1));
    let max_w = layout::detail_max_width(ui, &app.detail, left_w, addr_max_off);

    // 行宽/列开关变化后按新配置内容宽度自动收放右栏（面板本体只会被超宽内容
    // 撑到上限，内容变窄时不会自动缩回，需在 show 结束后改写 PanelState）
    let mut hex_pref_changed = false;
    let resp = egui::Panel::right("detail_panel")
        .default_size(360.0)
        .size_range(240.0..=max_w)
        .show(ui, |ui| {
            let Some(row_idx) = app.selected_row else {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.weak(t.detail_select_hint);
                });
                return;
            };
            let Some(row) = app.rows.get(row_idx) else {
                return;
            };
            let key = row.key.clone();
            let Some(value) = app.current_value() else {
                return;
            };
            let key_no = app.base_index.map(|b| b + row_idx + 1);
            let dup_sort = app.cur_table().map(|tbl| tbl.dup_sort).unwrap_or(false);

            hex_pref_changed = hex_toolbar(ui, app);
            ui.separator();

            // 预取两卡片的分段窗口与解码文本，用于估算自然高度
            let k_text = decode_card_preview(app, &key, Side::Key);
            let v_text = decode_card_preview(app, &value, Side::Value);
            let (kb, avail) = card_budget(ui, app, &key, &value, &k_text, &v_text, dup_sort);

            render_cards(
                ui, app, &key, &value, dup_sort, key_no, &k_text, &v_text, kb, avail,
            );
        });

    app.detail_panel_w = resp.response.rect.width();
    if hex_pref_changed {
        let limit =
            (ui.ctx().viewport_rect().width() - left_w - crate::ui::MIDDLE_MIN_WIDTH).max(240.0);
        let target = layout::detail_needed_width(ui, &app.detail, addr_max_off).clamp(240.0, limit);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(target, 0.0));
        ui.ctx().data_mut(|d| {
            d.insert_persisted(
                egui::Id::new("detail_panel"),
                egui::PanelState { outer_rect: rect },
            )
        });
    }
}

/// 顶部 hex 显示配置：三个列开关 + 行宽下拉（文字说明全部走 hover 提示）。
/// 返回 true 表示配置变化且已保存偏好（调用方据此收放右栏宽度）。
fn hex_toolbar(ui: &mut egui::Ui, app: &mut MdbxerApp) -> bool {
    let t = tr();
    let mut changed = false;
    ui.horizontal(|ui| {
        // HEX 与 ASCII 至少保留一项：只剩一项时该项变灰、不可取消；
        // 即只有另一项仍勾选时，才允许关掉这一项。
        changed = ui
            .checkbox(&mut app.detail.show_addr, t.addr)
            .on_hover_text(t.hex_addr_tip)
            .changed()
            | ui.add_enabled(
                app.detail.show_ascii,
                egui::Checkbox::new(&mut app.detail.show_hex, "HEX"),
            )
            .on_hover_text(t.hex_col_tip)
            .changed()
            | ui.add_enabled(
                app.detail.show_hex,
                egui::Checkbox::new(&mut app.detail.show_ascii, "ASCII"),
            )
            .on_hover_text(t.hex_ascii_tip)
            .changed();
        ui.separator();
        let mut w = app.detail.hex_width;
        let ir = egui::ComboBox::from_id_salt("hex_width")
            .width(52.0)
            .selected_text(w.to_string())
            .show_ui(ui, |ui| {
                for v in fmt::HEX_WIDTHS {
                    ui.selectable_value(&mut w, v, v.to_string());
                }
            });
        crate::ui::wheel_cycle(ui.ctx(), &ir.response, &fmt::HEX_WIDTHS, &mut w);
        ir.response.on_hover_text(t.hex_width_tip);
        if w != app.detail.hex_width {
            app.detail.hex_width = w;
            changed = true;
        }
    });
    if changed {
        app.save_ui_prefs();
    }
    changed
}

/// 卡片当前段已解码文本（高度估算与渲染共用同口径）。
fn decode_card_preview(app: &MdbxerApp, bytes: &[u8], side: Side) -> String {
    let (_, window) = layout::card_window(app, bytes, side);
    let mode = app.detail.mode_of(side);
    let max_chars = window.len() * 9 + 64;
    if side.is_key() {
        fmt::decode_key(window, mode, app.endian, max_chars, &app.hint_key)
    } else {
        fmt::decode(window, mode, app.endian, max_chars, &app.hint_value)
    }
}

/// 高度预算：两张卡片都装得下时各按自然高度；装不下时至少均分——
/// 小卡片按自然高度渲染，省下的余量全部让给大卡片。
/// 返回（Key 卡片预算, 进入滚动区前的可用高度）。
fn card_budget(
    ui: &egui::Ui,
    app: &MdbxerApp,
    key: &[u8],
    value: &[u8],
    k_text: &str,
    v_text: &str,
    dup_sort: bool,
) -> (f32, f32) {
    let (_, k_win) = layout::card_window(app, key, Side::Key);
    let (_, v_win) = layout::card_window(app, value, Side::Value);
    // 预算在进入外层滚动区之前量好（滚动区内 available_height 是"无限"）
    let avail = ui.available_height();
    let nk = layout::card_natural_height(ui, app, key.len(), k_win, k_text, Side::Key, false);
    let nv =
        layout::card_natural_height(ui, app, value.len(), v_win, v_text, Side::Value, dup_sort);

    let half = (avail - CARD_GAP) / 2.0;
    // Value 卡片的预算在 Key 卡片渲染完后按实测高度回填，这里只需 Key 份额
    let kb = if nk + CARD_GAP + nv <= avail || nk <= half {
        nk
    } else if nv <= half {
        avail - CARD_GAP - nv
    } else {
        half
    };
    (kb, avail)
}

/// 渲染两张卡片（外层滚动区仅作兜底，预算准确时不出现）。
#[allow(clippy::too_many_arguments)]
fn render_cards(
    ui: &mut egui::Ui,
    app: &mut MdbxerApp,
    key: &[u8],
    value: &[u8],
    dup_sort: bool,
    key_no: Option<usize>,
    k_text: &str,
    v_text: &str,
    kb: f32,
    avail: f32,
) {
    let t = tr();
    egui::ScrollArea::vertical().show(ui, |ui| {
        let key_title = match key_no {
            Some(n) => format!("Key #{n}"),
            None => "Key".to_string(),
        };
        let kh = card::show(ui, app, &key_title, key, Side::Key, key, k_text, kb);
        ui.add_space(CARD_GAP);

        // Key 卡片没用完的预算（估算偏差/内容少）全部实测回填给 Value，
        // 内容丰富时两卡恰好撑满右栏，底部不留空白
        let vb = (avail - CARD_GAP - kh).max(120.0);
        let val_title = if dup_sort {
            t.val_title(app.detail.dup.index + 1, app.detail.dup.total)
        } else {
            "Value".to_string()
        };
        card::show(ui, app, &val_title, value, Side::Value, key, v_text, vb);
    });
}

/// 显示文本内容指纹：折叠状态随文本变化自动失效用（换值/换段/换模式）。
fn text_fp(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}
