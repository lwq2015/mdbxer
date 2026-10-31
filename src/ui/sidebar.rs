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
            // 列表项和工具条按钮用的是 egui 的「按钮文字色」（深色主题 gray 180、
            // 浅色主题 gray 60），比正文文字色（深色 140 / 浅色 80）更亮/更暗，
            // 左栏因此和中部表格、右栏详情的字色不一致。这里把左栏内的按钮文字色
            // 统成正文色（只作用于本面板的作用域）。
            let text_color = ui.visuals().text_color();
            ui.style_mut().visuals.widgets.inactive.fg_stroke.color = text_color;

            let Some(_dbh) = &app.db else { return };

            // Tab 状态（持久化到 egui Memory）
            let tab_id = egui::Id::new("left_tab_fav");
            let is_fav = ui
                .ctx()
                .memory(|m| m.data.get_temp::<bool>(tab_id))
                .unwrap_or(false);

            // ── 底部 Tab 栏（VS 风格扁平 Tab，横向排列，锚定左栏最底部）──
            egui::Panel::bottom("left_tab_bar")
                .resizable(false)
                .show_separator_line(true)
                .show(ui, |ui| {
                    let h = ui.spacing().interact_size.y;
                    let w = ui.available_width() / 2.0;
                    let painter = ui.painter().clone();
                    let vis = ui.visuals().clone();
                    // 横向布局（左→右），让两个 Tab 并排
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), h),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            let tab1_resp =
                                ui.allocate_response(egui::vec2(w, h), egui::Sense::click());
                            let tab2_resp =
                                ui.allocate_response(egui::vec2(w, h), egui::Sense::click());
                            let tabs = [
                                (tab1_resp.rect, !is_fav, t.tables_title),
                                (tab2_resp.rect, is_fav, t.favorites_title),
                            ];
                            for (rect, active, label) in tabs {
                                // 选中态：顶部彩色线 + 略深背景
                                if active {
                                    painter.line_segment(
                                        [rect.left_top(), rect.right_top()],
                                        egui::Stroke::new(2.5, vis.selection.bg_fill),
                                    );
                                    painter.rect_filled(rect, 0.0, vis.panel_fill);
                                }
                                // hover 态背景
                                // 文字
                                let color = if active {
                                    vis.text_color()
                                } else {
                                    vis.weak_text_color()
                                };
                                painter.text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    label,
                                    egui::FontId::proportional(14.0),
                                    color,
                                );
                            }
                            if tab1_resp.clicked() {
                                ui.ctx().memory_mut(|m| m.data.insert_temp(tab_id, false));
                            }
                            if tab2_resp.clicked() {
                                ui.ctx().memory_mut(|m| m.data.insert_temp(tab_id, true));
                            }
                        },
                    );
                });

            // ── 内容区：占满 Tab 栏以上所有空间 ──
            ui.push_id("left_content", |ui| {
                if is_fav {
                    show_favorites(ui, app, &t);
                } else {
                    show_tables(ui, app, &t);
                }
            });
        });
    app.left_panel_w = resp.response.rect.width();
}

/// 表列表：排序下拉 + 过滤框 + 滚动列表
/// 导出按钮已迁移到顶栏。
fn show_tables(ui: &mut egui::Ui, app: &mut MdbxerApp, t: &crate::i18n::I18n) {
    let Some(dbh) = &app.db else { return };
    // 动作收集区
    let mut sort = app.table_sort;
    let mut filter_s = app.table_filter.clone();
    let mut clicked = None;
    let mut row_fav_toggled = None;

    // 排序下拉 + 过滤表名（同行）
    ui.horizontal(|ui| {
        // 左栏最窄可拖到 LEFT_PANEL_MIN：这一行两个控件的宽度都必须随可用宽度
        // 收缩。若给固定宽度或最小宽度，本行的最小宽度会超过面板宽度，egui 会把
        // 面板 rect 撑宽，但该帧背景已按撑宽前的宽度绘制，于是向右拖窄时左栏右缘
        // 露出一条窗口底色的黑缝（松手后下一帧背景才补上）。
        let avail = ui.available_width();
        let combo_w = (avail * 0.45).clamp(48.0, 80.0);
        let ir = egui::ComboBox::from_id_salt("table_sort")
            .width(combo_w)
            .selected_text(sort.label())
            .show_ui(ui, |ui| {
                for s in TableSort::ALL {
                    ui.selectable_value(&mut sort, s, s.label());
                }
            });
        super::wheel_cycle(ui.ctx(), &ir.response, &TableSort::ALL, &mut sort);
        let filter_w = ui.available_width();
        let fresp = ui.add(
            egui::TextEdit::singleline(&mut filter_s)
                .hint_text(t.filter_hint)
                .desired_width(filter_w),
        );
        // Esc：清空过滤词，还原完整表列表。egui 0.36 的无修饰 Esc 会在帧首
        // 没收焦点，正确判据是 lost_focus() && key_pressed(Escape)。
        if fresp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            filter_s.clear();
        }
    });
    ui.separator();

    // 过滤 + 排序 + 收藏表稳定置顶
    let filter = filter_s.to_lowercase();
    let mut idx: Vec<usize> = (0..dbh.tables.len()).collect();
    idx.retain(|&i| filter.is_empty() || dbh.tables[i].display().to_lowercase().contains(&filter));
    match sort {
        TableSort::NameAsc => {
            idx.sort_by(|&a, &b| dbh.tables[a].display().cmp(&dbh.tables[b].display()))
        }
        TableSort::NameDesc => {
            idx.sort_by(|&a, &b| dbh.tables[b].display().cmp(&dbh.tables[a].display()))
        }
        TableSort::CountAsc => idx.sort_by_key(|&i| dbh.tables[i].entries),
        TableSort::CountDesc => idx.sort_by_key(|&i| std::cmp::Reverse(dbh.tables[i].entries)),
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
                    let star_tip = if is_fav {
                        t.fav_rm_t_tip
                    } else {
                        t.fav_add_t_tip
                    };
                    if ui.small_button(star).on_hover_text(star_tip).clicked() {
                        row_fav_toggled = Some(tbl.name.clone());
                    }
                    let text = t.table_entry(&tbl.display(), tbl.entries, &tbl.flags_desc());
                    let resp = ui.add(
                        egui::Button::selectable(
                            selected,
                            egui::RichText::new(text.clone()).monospace(),
                        )
                        .truncate(),
                    );
                    let resp = super::dataview::button_tooltip(ui, resp, &text);
                    if resp.clicked() {
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
    if filter_s != app.table_filter {
        app.table_filter = filter_s;
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
    let bar_w = ui.spacing().scroll.bar_width;
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
                    .unwrap_or_else(|| fname.clone().unwrap_or_else(|| t.main_table.to_string()));
                let n_fav_keys = app.fav_keys.iter().filter(|f| &f.table == fname).count();
                display.push_str(&t.fav_count(n_fav_keys));
                let selected = app.cur_table().map(|ti| &ti.name) == Some(fname);
                let key_idx: Vec<usize> = app
                    .fav_keys
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| &f.table == fname)
                    .map(|(i, _)| i)
                    .collect();
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    egui::Id::new(("fav_fold", fname.clone())),
                    true,
                )
                .show_header(ui, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // 预留滚动条宽度，避免 x 按钮被挡
                        ui.allocate_space(egui::vec2(bar_w, 0.0));
                        // 右端 x 取消表收藏
                        if ui.small_button("x").on_hover_text(t.fav_rm_t_tip).clicked() {
                            fav_table_toggle = Some(fname.clone());
                        }
                        // 表名占剩余宽度
                        let resp = ui.add(
                            egui::Button::selectable(
                                selected,
                                egui::RichText::new(display.clone()).monospace(),
                            )
                            .truncate(),
                        );
                        let resp = super::dataview::button_tooltip(ui, resp, &display);
                        if resp.clicked() {
                            if let Some(pos) = dbh.tables.iter().position(|ti| ti.name == *fname) {
                                fav_table_pick = Some(pos);
                            }
                        }
                    });
                })
                .body_unindented(|ui| {
                    for i in key_idx {
                        let fk = &app.fav_keys[i];
                        let key_bytes = crate::ui::parse_hex(&fk.key_hex).ok();
                        let key_text = key_bytes
                            .as_deref()
                            .map(|bytes| {
                                super::detail::tooltip_key_text(
                                    bytes,
                                    crate::fmt::DecodeMode::Auto,
                                    app.endian,
                                    &app.hint_key,
                                    24,
                                )
                            })
                            .unwrap_or_else(|| fk.key_hex.chars().take(16).collect());
                        let label = if fk.note.is_empty() {
                            key_text
                        } else {
                            format!("{} · {key_text}", fk.note)
                        };
                        let key_mode = app.key_mode;
                        let endian = app.endian;
                        let hints = app.hint_key;
                        let jump_tip = t.fav_jump_tip;
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // 预留滚动条宽度
                                    ui.allocate_space(egui::vec2(bar_w, 0.0));
                                    // 右端 x 取消 Key 收藏
                                    if ui.small_button("x").on_hover_text(t.fav_rm_k_tip).clicked()
                                    {
                                        fav_key_action = Some((i, false));
                                    }
                                    // Key 名占剩余宽度
                                    if ui
                                        .add(
                                            egui::Button::selectable(
                                                false,
                                                egui::RichText::new(label).monospace(),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_ui(move |ui| {
                                            let key_text = key_bytes
                                                .as_deref()
                                                .map(|bytes| {
                                                    super::detail::tooltip_key_text(
                                                        bytes,
                                                        key_mode,
                                                        endian,
                                                        &hints,
                                                        super::dataview::TOOLTIP_CHARS,
                                                    )
                                                })
                                                .unwrap_or_else(|| {
                                                    fk.key_hex.chars().take(16).collect()
                                                });
                                            super::dataview::tooltip_body(
                                                ui,
                                                &format!("{jump_tip}\n\n{key_text}"),
                                            );
                                        })
                                        .clicked()
                                    {
                                        fav_key_action = Some((i, true));
                                    }
                                },
                            );
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
