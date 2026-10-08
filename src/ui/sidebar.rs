// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 左侧表（subDB）列表：Tab 切换「表」和「收藏」。
//! Tab 在底部，内容区占满上方可用高度。

use super::{LEFT_PANEL_MAX, LEFT_PANEL_MIN, MIDDLE_MIN_WIDTH, MdbxerApp, TableSort};
use crate::i18n::tr;

/// 左侧面板：底部 Tab 切换 表列表 / 收藏区。
pub fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let t = tr();
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
            let Some(dbh) = &app.db else { return };

            // Tab 状态（持久化到 egui Memory）
            let tab_id = egui::Id::new("left_tab_fav");
            let is_fav = ui.ctx().memory(|m| m.data.get_temp::<bool>(tab_id)).unwrap_or(false);

            // ── 内容区：占满除 Tab 栏外的所有空间（固定高度，Tab 栏不跳）──
            let tab_bar_h = ui.spacing().interact_size.y + 4.0; // 按钮+separator
            let content_h = (ui.available_height() - tab_bar_h).max(60.0);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), content_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if is_fav {
                        show_favorites(ui, app, &t);
                    } else {
                        show_tables(ui, app, &t);
                    }
                },
            );

            // ── 底部 Tab 栏 ──
            ui.separator();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let w = ui.available_width() / 2.0;
                if ui.add(
                    egui::Button::selectable(!is_fav, egui::RichText::new(t.tables_title).strong())
                        .min_size(egui::vec2(w, 0.0)),
                ).clicked() {
                    ui.ctx().memory_mut(|m| m.data.insert_temp(tab_id, false));
                }
                if ui.add(
                    egui::Button::selectable(is_fav, egui::RichText::new(t.favorites_title).strong())
                        .min_size(egui::vec2(w, 0.0)),
                ).clicked() {
                    ui.ctx().memory_mut(|m| m.data.insert_temp(tab_id, true));
                }
            });
        });
    app.left_panel_w = resp.response.rect.width();
}

/// 表列表：排序下拉 + 过滤框 + 导出 + 滚动列表
fn show_tables(ui: &mut egui::Ui, app: &mut MdbxerApp, t: &crate::i18n::I18n) {
    let Some(dbh) = &app.db else { return };
    // 动作收集区
    let mut sort = app.table_sort;
    let mut ef = app.export_format;
    let mut filter_s = app.table_filter.clone();
    let mut export_clicked = false;
    let mut clicked = None;
    let mut row_fav_toggled = None;

    // 排序下拉 + 导出
    ui.horizontal(|ui| {
        let ir = egui::ComboBox::from_id_salt("table_sort")
            .width(80.0)
            .selected_text(sort.label())
            .show_ui(ui, |ui| {
                for s in TableSort::ALL {
                    ui.selectable_value(&mut sort, s, s.label());
                }
            });
        super::wheel_cycle(ui.ctx(), &ir.response, &TableSort::ALL, &mut sort);
    });
    // 过滤表名 + 导出（right_to_left 布局）
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
            egui::ComboBox::from_id_salt("export_format")
                .width(52.0)
                .selected_text(ef.label())
                .show_ui(ui, |ui| {
                    for f in crate::export::ExportFormat::ALL {
                        ui.selectable_value(&mut ef, f, f.label());
                    }
                });
            let filter_w = ui.available_width().max(40.0);
            ui.add(
                egui::TextEdit::singleline(&mut filter_s)
                    .hint_text(t.filter_hint)
                    .desired_width(filter_w),
            );
        });
    });
    ui.separator();

    // 过滤 + 排序 + 收藏表稳定置顶
    let filter = filter_s.to_lowercase();
    let mut idx: Vec<usize> = (0..dbh.tables.len()).collect();
    idx.retain(|&i| {
        filter.is_empty() || dbh.tables[i].display().to_lowercase().contains(&filter)
    });
    match sort {
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
    idx.sort_by_key(|&i| !app.fav_tables.contains(&dbh.tables[i].name));

    // ScrollArea 占满剩余高度
    ui.spacing_mut().item_spacing = egui::vec2(4.0, 1.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for i in idx {
                let tbl = &dbh.tables[i];
                let selected = app.selected_table == Some(i);
                let is_fav = app.fav_tables.contains(&tbl.name);
                ui.horizontal(|ui| {
                    let star = if is_fav { "★" } else { "☆" };
                    let star_tip = if is_fav { t.fav_rm_t_tip } else { t.fav_add_t_tip };
                    if ui.small_button(star).on_hover_text(star_tip).clicked() {
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

    // 统一写回
    if sort != app.table_sort {
        app.table_sort = sort;
        app.save_ui_prefs();
    }
    if ef != app.export_format {
        app.export_format = ef;
        app.save_ui_prefs();
    }
    if filter_s != app.table_filter {
        app.table_filter = filter_s;
    }
    if export_clicked {
        app.start_export();
    }
    if let Some(i) = clicked {
        app.select_table(i);
    }
    if let Some(name) = row_fav_toggled {
        app.toggle_fav_table(name);
    }
}

/// 收藏区：收藏的表（可折叠展开 Key 子列表）
fn show_favorites(ui: &mut egui::Ui, app: &mut MdbxerApp, t: &crate::i18n::I18n) {
    let Some(dbh) = &app.db else { return };
    let mut fav_table_pick: Option<usize> = None;
    let mut fav_table_toggle: Option<Option<String>> = None;
    let mut fav_key_action: Option<(usize, bool)> = None;

    ui.spacing_mut().item_spacing = egui::vec2(4.0, 1.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if app.fav_tables.is_empty() {
                ui.weak(t.fav_empty);
            }
            for fname in &app.fav_tables {
                let mut display = dbh
                    .tables
                    .iter()
                    .find(|ti| &ti.name == fname)
                    .map(|ti| ti.display())
                    .unwrap_or_else(|| {
                        fname.clone().unwrap_or_else(|| t.main_table.to_string())
                    });
                let n_fav_keys = app
                    .fav_keys
                    .iter()
                    .filter(|f| &f.table == fname)
                    .count();
                display.push_str(&t.fav_count(n_fav_keys));
                let selected = app.cur_table().map(|ti| &ti.name) == Some(fname);
                let key_idx: Vec<usize> = app
                    .fav_keys
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| &f.table == fname)
                    .map(|(i, _)| i)
                    .collect();
                egui::collapsing_header::CollapsingState
                    ::load_with_default_open(
                        ui.ctx(),
                        egui::Id::new(("fav_fold", fname.clone())),
                        true,
                    )
                    .show_header(ui, |ui| {
                        if ui
                            .small_button("★")
                            .on_hover_text(t.fav_rm_t_tip)
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
                            if let Some(pos) = dbh
                                .tables
                                .iter()
                                .position(|ti| ti.name == *fname)
                            {
                                fav_table_pick = Some(pos);
                            }
                        }
                    })
                    .body_unindented(|ui| {
                        for i in key_idx {
                            let fk = &app.fav_keys[i];
                            ui.horizontal(|ui| {
                                if ui
                                    .small_button("★")
                                    .on_hover_text(t.fav_rm_k_tip)
                                    .clicked()
                                {
                                    fav_key_action = Some((i, false));
                                }
                                let key_text =
                                    crate::ui::parse_hex(&fk.key_hex)
                                        .map(|b| {
                                            crate::fmt::decode(
                                                &b,
                                                crate::fmt::DecodeMode::Auto,
                                                app.endian,
                                                24,
                                            )
                                        })
                                        .unwrap_or_else(|_| {
                                            fk.key_hex.chars().take(16).collect()
                                        });
                                let label = if fk.note.is_empty() {
                                    key_text
                                } else {
                                    format!("{} · {key_text}", fk.note)
                                };
                                if ui
                                    .add(
                                        egui::Button::selectable(
                                            false,
                                            egui::RichText::new(label).monospace(),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(format!("{}\n{}", t.fav_jump_tip, fk.key_hex))
                                    .clicked()
                                {
                                    fav_key_action = Some((i, true));
                                }
                            });
                        }
                    });
            }
        });

    // 头部 ★ 取消表收藏（放在 CollapsingHeader 外面，因为 header 内不好同时放 ★ 和标题）
    // 实际上上面用了 CollapsingHeader::show_unindented，标题就是表名+数量
    // ★ 取消收藏改为悬停在表名上时显示提示，点击表名跳转
    // 这里处理跳转和取消收藏
    if let Some(i) = fav_table_pick {
        app.select_table(i);
    }
    if let Some(name) = fav_table_toggle {
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
}
