// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 左侧表（subDB）列表：过滤、排序、条数显示；收藏区（收藏表 + 收藏 Key）。
//! 两个区域各自独立折叠，折叠状态决定高度分配：
//! 都展开→平分；其一折叠→另一个占满。

use super::{LEFT_PANEL_MAX, LEFT_PANEL_MIN, MIDDLE_MIN_WIDTH, MdbxerApp, TableSort};
use crate::i18n::tr;

/// 左侧表列表面板：表列表 + 收藏区，两个独立折叠区。
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
            let Some(dbh) = &app.db else { return };

            // ── 读取两个折叠区状态，据此分配高度 ──
            let mut tables_state = egui::collapsing_header::CollapsingState
                ::load_with_default_open(ui.ctx(), egui::Id::new("tables_fold"), true);
            let mut fav_state = egui::collapsing_header::CollapsingState
                ::load_with_default_open(ui.ctx(), egui::Id::new("fav_keys"), true);
            let tables_open = tables_state.is_open();
            let fav_open = fav_state.is_open();

            // 两个 header 各占一行（约 24px），内容区平分剩余高度
            let avail_h = ui.available_height();
            let header_h = 24.0;
            let remaining = (avail_h - 2.0 * header_h).max(60.0);
            let tables_max = if tables_open {
                if fav_open { remaining / 2.0 } else { remaining }
            } else {
                0.0
            };
            let fav_max = if fav_open {
                if tables_open { remaining / 2.0 } else { remaining }
            } else {
                0.0
            };

            // 动作收集区（闭包结束后统一应用，规避借用）
            let mut fav_table_pick: Option<usize> = None;
            let mut fav_table_toggle: Option<Option<String>> = None;
            let mut fav_key_action: Option<(usize, bool)> = None;
            // 表列表镜像
            let mut sort = app.table_sort;
            let mut ef = app.export_format;
            let mut filter_s = app.table_filter.clone();
            let mut export_clicked = false;
            let mut clicked = None;
            let mut row_fav_toggled = None;

            // ── 表列表区：三角 + 标题（可点击折叠）+ 排序下拉，同一行 ──
            ui.horizontal(|ui| {
                // 折叠三角（与详情区同款矢量三角，egui 内置绘制）
                let openness = tables_state.openness(ui.ctx());
                let (rect, tri_resp) = ui.allocate_exact_size(
                    egui::vec2(12.0, ui.spacing().interact_size.y),
                    egui::Sense::click(),
                );
                egui::collapsing_header::paint_default_icon(ui, openness, &tri_resp);
                if tri_resp.clicked() {
                    tables_state.toggle(ui);
                }
                let _ = rect; // rect 已被 paint 使用
                // 标题（点击折叠，和 CollapsingHeader 一致）
                let title_resp = ui.add(
                    egui::Button::new(
                        egui::RichText::new(t.tables_title).strong(),
                    )
                    .frame(false),
                );
                if title_resp.clicked() {
                    tables_state.toggle(ui);
                }
                // 排序下拉紧跟标题
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

            // 表列表 body（不缩进）
            tables_state.show_body_unindented(ui, |ui| {
                // 过滤表名 + 导出：right_to_left 布局
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
                    filter.is_empty()
                        || dbh.tables[i].display().to_lowercase().contains(&filter)
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

                egui::ScrollArea::vertical()
                    .max_height(tables_max)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for i in idx {
                            let tbl = &dbh.tables[i];
                            let selected = app.selected_table == Some(i);
                            let is_fav = app.fav_tables.contains(&tbl.name);
                            ui.horizontal(|ui| {
                                let star = if is_fav { "★" } else { "☆" };
                                let star_tip = if is_fav { t.fav_rm_t_tip } else { t.fav_add_t_tip };
                                if ui
                                    .small_button(star)
                                    .on_hover_text(star_tip)
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
            });

            // ── 收藏区：三角 + 标题（可点击折叠），同一行 ──
            ui.horizontal(|ui| {
                let openness = fav_state.openness(ui.ctx());
                let (rect, tri_resp) = ui.allocate_exact_size(
                    egui::vec2(12.0, ui.spacing().interact_size.y),
                    egui::Sense::click(),
                );
                egui::collapsing_header::paint_default_icon(ui, openness, &tri_resp);
                if tri_resp.clicked() {
                    fav_state.toggle(ui);
                }
                let _ = rect;
                let title_resp = ui.add(
                    egui::Button::new(
                        egui::RichText::new(t.favorites_title).strong(),
                    )
                    .frame(false),
                );
                if title_resp.clicked() {
                    fav_state.toggle(ui);
                }
            });

            // 收藏区 body（不缩进）
            fav_state.show_body_unindented(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 1.0);
                egui::ScrollArea::vertical()
                    .max_height(fav_max)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        if app.fav_tables.is_empty() {
                            ui.weak(t.fav_empty);
                        }
                        // 收藏的表：折叠头，body 缩进渲染 Key 收藏子列表
                        for fname in &app.fav_tables {
                            let mut display = dbh
                                .tables
                                .iter()
                                .find(|ti| &ti.name == fname)
                                .map(|ti| ti.display())
                                .unwrap_or_else(|| {
                                    fname
                                        .clone()
                                        .unwrap_or_else(|| t.main_table.to_string())
                                });
                            let n_fav_keys = app
                                .fav_keys
                                .iter()
                                .filter(|f| &f.table == fname)
                                .count();
                            display.push_str(&t.fav_count(n_fav_keys));
                            let selected =
                                app.cur_table().map(|ti| &ti.name) == Some(fname);
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
                                .body(|ui| {
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
                                                .on_hover_text(format!(
                                                    "{}\n{}",
                                                    t.fav_jump_tip, fk.key_hex
                                                ))
                                                .clicked()
                                            {
                                                fav_key_action = Some((i, true));
                                            }
                                        });
                                    }
                                });
                        }
                    });
            });

            // ── 统一写回镜像并应用本帧动作（闭包借用到此全部结束）──
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
