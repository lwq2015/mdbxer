// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 卡片标题行：收藏、复制、另存、格式下拉，以及 Protobuf 类型选择行。

use super::super::export;
use super::super::state::Side;
use crate::fmt::{self, DecodeMode};
use crate::i18n::tr;
use crate::ui::syntax::syntax_mode;
use crate::ui::{MdbxerApp, format_groups, wheel_cycle};

/// 标题行：标题 + 收藏（仅 Key）+ 右侧的复制/另存/格式下拉。
pub(super) fn title_row(
    ui: &mut egui::Ui,
    app: &mut MdbxerApp,
    side: Side,
    title: &str,
    bytes: &[u8],
    save_base: &str,
) {
    ui.horizontal(|ui| {
        ui.strong(title);
        if side.is_key() {
            favorite_button(ui, app, bytes);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            copy_button(ui, app, side, bytes);
            save_menu(ui, app, side, bytes, save_base);
            mode_combo(ui, app, side);
        });
    });
}

/// Key 卡片：收藏 ☆/★（点击切换，立即持久化）。
fn favorite_button(ui: &mut egui::Ui, app: &mut MdbxerApp, key: &[u8]) {
    let t = tr();
    let is_fav = app.is_fav_key(key);
    let star = if is_fav { "★" } else { "☆" };
    let star_tip = if is_fav {
        t.fav_rm_k_tip
    } else {
        t.fav_add_k_tip
    };
    if ui.small_button(star).on_hover_text(star_tip).clicked() {
        app.toggle_fav_key(key);
    }
}

/// 复制：与详情显示同口径（Auto 下压缩穿透/结构化/proto 均按 pretty 复制）。
fn copy_button(ui: &mut egui::Ui, app: &mut MdbxerApp, side: Side, bytes: &[u8]) {
    let mode = app.detail.mode_of(side);
    if !ui.button(tr().copy).clicked() {
        return;
    }

    // Auto 下压缩容器：解压后内层为结构化 JSON/BSON/MSGPACK 时按内层格式复制 pretty
    let decomp_copy = (mode == DecodeMode::Auto)
        .then(|| fmt::decompress(bytes))
        .flatten()
        .and_then(|(_, inner)| syntax_mode(DecodeMode::Auto, &inner).map(|m| (m, inner)));
    if let Some((m, inner)) = decomp_copy {
        ui.ctx().copy_text(fmt::decode(
            &inner,
            m,
            app.endian,
            usize::MAX,
            &app.hint_value,
        ));
        return;
    }

    // Auto + 已载入协议池：试探出 message 类型时按该类型复制 proto3 JSON pretty
    if !side.is_key()
        && let Some(pretty) = export::proto_auto_pretty(mode, bytes)
    {
        ui.ctx().copy_text(pretty);
        return;
    }

    // Auto 下完整值是结构化格式时按对应格式复制 pretty；其余按当前实际模式复制。
    // Key 不做特殊格式猜测（与 Auto 猜型策略一致），Auto 直接按原文复制。
    let copy_mode = match mode {
        DecodeMode::Auto if !side.is_key() => {
            syntax_mode(DecodeMode::Auto, bytes).unwrap_or(DecodeMode::Auto)
        }
        m => m,
    };
    let text = export::text_of(bytes, copy_mode, app.endian, hints(app, side), side);
    ui.ctx().copy_text(text);
}

/// 另存菜单：原始二进制 / 可见文本 / 猜测格式（Auto 且命中时）。
fn save_menu(ui: &mut egui::Ui, app: &mut MdbxerApp, side: Side, bytes: &[u8], save_base: &str) {
    let t = tr();
    ui.menu_button(t.save_as, |ui| {
        if ui.button(t.save_raw).clicked() {
            ui.close();
            app.save_bytes(&format!("{save_base}.bin"), bytes, "bin");
        }

        if ui.button(t.save_text).clicked() {
            ui.close();
            let text = export::text_of(
                bytes,
                app.detail.mode_of(side),
                app.endian,
                hints(app, side),
                side,
            );
            app.save_bytes(&format!("{save_base}.txt"), text.as_bytes(), "txt");
        }

        // 猜测格式：仅 Value 且 Auto 模式（Key 不做特殊格式猜测，
        // 与猜型策略一致；手动模式下"可见文本"已是该格式文本）
        if !side.is_key()
            && app.detail.mode_of(side) == DecodeMode::Auto
            && let Some((label, data, ext)) = export::guess_save(bytes)
            && ui.button(format!("{label}（.{ext}）")).clicked()
        {
            ui.close();
            app.save_bytes(&format!("{save_base}.{ext}"), &data, ext);
        }
    })
    .response
    .on_hover_text(t.save_tip);
}

/// 格式下拉：自动项置顶，其余按"文本编码/数值/字节视图/标识与时间/结构化"分组。
/// 详情卡片排版独立于顶栏表格排版：只改本卡片，不回写全局、不存偏好。
fn mode_combo(ui: &mut egui::Ui, app: &mut MdbxerApp, side: Side) {
    let mut mode = app.detail.mode_of(side);
    let flat = DecodeMode::flat_all();
    let ir = egui::ComboBox::from_id_salt(("detail_mode", side.is_key()))
        .selected_text(mode.label())
        .height(430.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut mode, DecodeMode::Auto, DecodeMode::Auto.label());
            for (grp, items) in format_groups() {
                // 组标题：弱化小字，不可选中
                ui.add(egui::Label::new(egui::RichText::new(grp).weak().small()));
                for m in items {
                    ui.selectable_value(&mut mode, *m, m.label());
                }
            }
        });
    wheel_cycle(ui.ctx(), &ir.response, &flat, &mut mode);
    if app.detail.mode_of(side) != mode {
        app.detail.set_mode(side, mode);
    }
}

/// Protobuf 行：message 类型选择 + .proto 载入。类型池与当前选择是全局的
/// （Key/Value 两卡共用、重启沿用），仅在选择 proto 排版时显示本行。
pub(super) fn proto_row(ui: &mut egui::Ui, app: &mut MdbxerApp, side: Side) {
    ui.horizontal(|ui| {
        proto_type_combo(ui, app, side);
        proto_load_button(ui, app);
        proto_clear_button(ui, app);
        proto_error(ui, app);
    });
}

/// message 类型下拉（全名 pkg.Msg；空池显示占位项）。
fn proto_type_combo(ui: &mut egui::Ui, app: &mut MdbxerApp, side: Side) {
    let t = tr();
    let names = crate::proto::message_names();
    let mut cur = crate::proto::current();
    let sel_text = cur.clone().unwrap_or_else(|| t.proto_pick.to_string());

    let ir = egui::ComboBox::from_id_salt(("proto_type", side.is_key()))
        .selected_text(sel_text)
        .width(180.0)
        .height(430.0)
        .show_ui(ui, |ui| {
            if names.is_empty() {
                ui.add_enabled(false, egui::Label::new(t.proto_no_types));
            }
            for n in &names {
                if ui.selectable_value(&mut cur, Some(n.clone()), n).changed() {
                    crate::proto::set_current(cur.clone());
                    export::save_proto_binding(app, cur.as_deref());
                }
            }
        });
    ir.response.on_hover_text(t.proto_pick);
}

/// 载入 .proto（可多选；失败保留旧池并显示错误）。
fn proto_load_button(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let t = tr();
    let resp = ui
        .small_button(t.proto_load)
        .on_hover_text(t.proto_load_tip);
    if !resp.clicked() {
        return;
    }
    let Some(paths) = rfd::FileDialog::new()
        .add_filter("Protobuf", &["proto"])
        .pick_files()
    else {
        return;
    };
    match crate::proto::load_files(&paths) {
        Ok(_) => {
            app.detail.proto_err = None;
            export::save_proto_binding(app, crate::proto::current().as_deref());
        }
        Err(e) => app.detail.proto_err = Some(e),
    }
}

/// 清除关联：删除本库的 .proto 绑定并清空载入状态。
fn proto_clear_button(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let t = tr();
    let resp = ui
        .small_button(t.proto_clear)
        .on_hover_text(t.proto_clear_tip);
    if !resp.clicked() {
        return;
    }
    if let Some(p) = &app.opened_path {
        crate::config::clear_proto_binding(p);
    }
    crate::proto::clear();
    app.detail.proto_err = None;
}

/// 载入失败提示：截断显示，hover 看全文。
fn proto_error(ui: &mut egui::Ui, app: &MdbxerApp) {
    if let Some(e) = &app.detail.proto_err {
        let short: String = e.chars().take(60).collect();
        ui.label(egui::RichText::new(format!("⚠ {short}")).small().weak())
            .on_hover_text(e);
    }
}

/// 当前卡片角色对应的解码提示开关。
fn hints(app: &MdbxerApp, side: Side) -> &fmt::HintFlags {
    if side.is_key() {
        &app.hint_key
    } else {
        &app.hint_value
    }
}
