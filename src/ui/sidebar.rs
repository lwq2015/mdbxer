// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 左侧表（subDB）列表：过滤、排序、条数显示；底部固定收藏区（收藏表 + 收藏 Key）。

use super::{LEFT_PANEL_MAX, LEFT_PANEL_MIN, MIDDLE_MIN_WIDTH, MdbxerApp, TableSort};
use crate::i18n::tr;

/// 左侧表列表面板：标题 + 排序下拉 + 过滤框 + 可滚动列表 + 底部收藏区。
pub fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let t = tr();
    // 上限取硬上限与"给右栏+中央表格留足宽度"两者中的较小值
    let screen = ui.ctx().viewport_rect().width();
    let right_w = if app.detail_visible {
        app.detail_panel_w
    } else {
        0.0
    };
    let max_w = (screen - right_w - MIDDLE_MIN_WIDTH).clamp(LEFT_PANEL_MIN, LEFT_PANEL_MAX);
    let resp = egui::Panel::left("table_list")
        .default_size(220.0)
        .resizable(true)
        .size_range(LEFT_PANEL_MIN..=max_w)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(t.tables_title);
                ui.separator();
                let mut sort = app.table_sort;
                let ir = egui::ComboBox::from_id_salt("table_sort")
                    .width(86.0)
                    .selected_text(sort.label())
                    .show_ui(ui, |ui| {
                        for s in TableSort::ALL {
                            ui.selectable_value(&mut sort, s, s.label());
                        }
                    });
                super::wheel_cycle(ui.ctx(), &ir.response, &TableSort::ALL, &mut sort);
                if sort != app.table_sort {
                    app.table_sort = sort;
                    app.save_ui_prefs();
                }
            });
            // 过滤表名（弹性占满剩余宽度）+ 导出当前表（CSV/JSON 下拉 + ⬇）
            // ⬇ U+2B07 取自 egui 内置 NotoEmoji/emoji-icon，跨平台一致
            ui.horizontal(|ui| {
                // 先算出导出控件所需宽度，剩余空间给过滤框，避免过滤框 INFINITY
                // 把导出挤出可见区
                let spacing = ui.spacing().item_spacing.x;
                let export_w = 52.0 // ComboBox 宽度
                    + 26.0 // ⬇ 按钮宽度
                    + spacing * 2.0; // 两个控件间距
                let filter_w = (ui.available_width() - export_w).max(40.0);
                ui.add(
                    egui::TextEdit::singleline(&mut app.table_filter)
                        .hint_text(t.filter_hint)
                        .desired_width(filter_w),
                );
                let mut export_clicked = false;
                let mut ef = app.export_format;
                egui::ComboBox::from_id_salt("export_format")
                    .width(52.0)
                    .selected_text(ef.label())
                    .show_ui(ui, |ui| {
                        for f in crate::export::ExportFormat::ALL {
                            ui.selectable_value(&mut ef, f, f.label());
                        }
                    });
                if ef != app.export_format {
                    app.export_format = ef;
                    app.save_ui_prefs();
                }
                if ui
                    .add_enabled(
                        app.db.is_some() && app.export_ev_rx.is_none(),
                        egui::Button::new("⬇"),
                    )
                    .on_hover_text(t.export_tip)
                    .clicked()
                {
                    export_clicked = true;
                }
                if export_clicked {
                    app.start_export();
                }
            });
            ui.separator();

            let Some(dbh) = &app.db else { return };

            // ── 底部固定收藏区（先登记面板，剩余空间才全部分给表列表）──
            // 动作在不可变收集区记录，闭包结束后统一应用（规避借用）
            let mut fav_table_pick: Option<usize> = None;
            let mut fav_table_toggle: Option<Option<String>> = None;
            let mut fav_key_action: Option<(usize, bool)> = None; // (下标, true=跳转)
            egui::Panel::bottom("fav_panel")
                .resizable(false)
                // 只清左右内边距（默认 symmetric(8,2) 与父面板 padding 叠加显得太宽），
                // 保留 panel_fill 背景与顶部分隔线
                .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(0, 2)))
                .show(ui, |ui| {
                    // 不在此再加 ui.separator()：bottom 面板自身已在顶边画分隔线
                    egui::CollapsingHeader::new(t.favorites_title)
                        .id_salt("fav_keys")
                        .default_open(true)
                        .show_unindented(ui, |ui| {
                            // 收紧行内间距：★/➡/× 按钮与文字贴紧，左右都不留多余空隙
                            ui.spacing_mut().item_spacing = egui::vec2(4.0, 1.0);
                            if app.fav_tables.is_empty() && app.fav_keys.is_empty() {
                                ui.weak(t.fav_empty);
                            }
                            // ── 收藏的表：★ 取消收藏，点表名直接跳转 ──
                            for fname in &app.fav_tables {
                                let display = dbh
                                    .tables
                                    .iter()
                                    .find(|ti| &ti.name == fname)
                                    .map(|ti| ti.display())
                                    .unwrap_or_else(|| {
                                        fname.clone().unwrap_or_else(|| t.main_table.to_string())
                                    });
                                let selected = app.cur_table().map(|ti| &ti.name) == Some(fname);
                                ui.horizontal(|ui| {
                                    if ui
                                        .small_button("★")
                                        .on_hover_text(t.fav_table_tip)
                                        .clicked()
                                    {
                                        fav_table_toggle = Some(fname.clone());
                                    }
                                    if ui
                                        .add(
                                            egui::Button::selectable(
                                                selected,
                                                egui::RichText::new(display).monospace(),
                                            )
                                            .truncate(),
                                        )
                                        .clicked()
                                    {
                                        if let Some(pos) =
                                            dbh.tables.iter().position(|ti| ti.name == *fname)
                                        {
                                            fav_table_pick = Some(pos);
                                        }
                                    }
                                });
                            }
                            // ── 收藏的 Key：× 删除，点标签直接跳转 ──
                            for (i, fk) in app.fav_keys.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    if ui.small_button("×").on_hover_text(t.fav_del_tip).clicked()
                                    {
                                        fav_key_action = Some((i, false));
                                    }
                                    let table_name = fk.table.as_deref().unwrap_or(t.main_table);
                                    // Key 显示解码后的可读文本（自动猜测），hex 全文放 hover
                                    let key_text = crate::ui::parse_hex(&fk.key_hex)
                                        .map(|b| {
                                            crate::fmt::decode(
                                                &b,
                                                crate::fmt::DecodeMode::Auto,
                                                app.endian,
                                                24,
                                            )
                                        })
                                        .unwrap_or_else(|_| fk.key_hex.chars().take(16).collect());
                                    let label = if fk.note.is_empty() {
                                        format!("{table_name} · {key_text}")
                                    } else {
                                        format!("{table_name} · {} · {key_text}", fk.note)
                                    };
                                    if ui
                                        .add(
                                            egui::Button::selectable(
                                                false,
                                                egui::RichText::new(label).monospace(),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(format!(
                                            "{}\n{table_name} · {}",
                                            t.fav_jump_tip, fk.key_hex
                                        ))
                                        .clicked()
                                    {
                                        fav_key_action = Some((i, true));
                                    }
                                });
                            }
                        });
                });

            // ── 表列表（过滤 + 排序 + 收藏表稳定置顶）──
            let filter = app.table_filter.to_lowercase();
            let mut idx: Vec<usize> = (0..dbh.tables.len()).collect();
            idx.retain(|&i| {
                filter.is_empty() || dbh.tables[i].display().to_lowercase().contains(&filter)
            });
            match app.table_sort {
                TableSort::NameAsc => {
                    idx.sort_by(|&a, &b| dbh.tables[a].display().cmp(&dbh.tables[b].display()))
                }
                TableSort::NameDesc => {
                    idx.sort_by(|&a, &b| dbh.tables[b].display().cmp(&dbh.tables[a].display()))
                }
                TableSort::CountAsc => idx.sort_by_key(|&i| dbh.tables[i].entries),
                TableSort::CountDesc => {
                    idx.sort_by_key(|&i| std::cmp::Reverse(dbh.tables[i].entries))
                }
            }
            // 收藏表稳定置顶（保持各自原有的排序结果顺序）
            idx.sort_by_key(|&i| !app.fav_tables.contains(&dbh.tables[i].name));

            let mut clicked = None;
            let mut row_fav_toggled = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for i in idx {
                    let tbl = &dbh.tables[i];
                    let selected = app.selected_table == Some(i);
                    let is_fav = app.fav_tables.contains(&tbl.name);
                    ui.horizontal(|ui| {
                        let star = if is_fav { "★" } else { "☆" };
                        if ui
                            .small_button(star)
                            .on_hover_text(t.fav_table_tip)
                            .clicked()
                        {
                            row_fav_toggled = Some(tbl.name.clone());
                        }
                        let text = t.table_entry(&tbl.display(), tbl.entries, &tbl.flags_desc());
                        if ui
                            .add(
                                egui::Button::selectable(
                                    selected,
                                    egui::RichText::new(text).monospace(),
                                )
                                .truncate(),
                            )
                            .clicked()
                        {
                            clicked = Some(i);
                        }
                    });
                }
            });

            // ── 统一应用本帧动作（dbh 的不可变借用到此结束）──
            if let Some(i) = clicked.or(fav_table_pick) {
                app.select_table(i);
            }
            if let Some(name) = row_fav_toggled.or(fav_table_toggle) {
                app.toggle_fav_table(name);
            }
            if let Some((i, jump)) = fav_key_action {
                if jump {
                    let fk = app.fav_keys[i].clone();
                    app.jump_to_fav(&fk);
                } else {
                    app.remove_fav_key(i);
                }
            }
        });
    app.left_panel_w = resp.response.rect.width();
}
