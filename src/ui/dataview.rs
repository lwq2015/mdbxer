// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 中间 "数据" 页签：工具条 + 表格。

use super::{MdbxerApp, SortCol};
use crate::i18n::tr;
use egui_extras::{Column, TableBuilder};

/// 列头单元格：文字与整列空白都可点击。返回是否被点击。
fn header_cell(ui: &mut egui::Ui, text: &str, tip: &str) -> bool {
    let r1 = ui
        .add(egui::Label::new(egui::RichText::new(text).strong()).sense(egui::Sense::click()))
        .on_hover_text(tip);
    // 覆盖列内剩余空白区域，使整列都可点击、可悬停看提示
    let r2 = ui
        .allocate_rect(ui.available_rect_before_wrap(), egui::Sense::click())
        .on_hover_text(tip);
    r1.clicked() || r2.clicked()
}

/// 单元格 hover 提示：仅当内容被截断时弹，强制默认颜色避免选中行白字白底。
fn elide_tooltip(ui: &mut egui::Ui, resp: egui::Response, full: &str) {
    let mono = egui::FontId::monospace(ui.style().text_styles[&egui::TextStyle::Body].size);
    let width = ui.fonts_mut(|f| {
        f.layout_no_wrap(full.to_string(), mono, egui::Color32::WHITE)
            .size()
            .x
    });
    if width > resp.rect.width() {
        let text = if full.chars().count() > 512 {
            full.chars().take(512).collect::<String>() + "…"
        } else {
            full.to_string()
        };
        resp.on_hover_text(
            egui::RichText::new(text)
                .monospace()
                .color(ui.visuals().text_color()),
        );
    }
}

/// "数据"页签入口：工具条 + 行表格 + 列头排序处理。
pub fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let t = tr();
    if app.cur_table().is_none() {
        ui.label(t.select_table);
        return;
    }

    // ── 工具条（单行紧凑：符号按钮 + 悬停说明；范围信息见底部状态栏）──
    // 每页条数在顶栏；导出当前表在左栏表区域；Key 跳转/过滤统一到搜索框
    ui.horizontal(|ui| {
        // 默认顺序 = 表中读取出来的顺序（正向遍历）
        let sort_text = match app.col_sort {
            Some((col, asc)) => {
                format!("{} {}", col.label(), if asc { "⬆" } else { "⬇" })
            }
            None => {
                if app.sort_desc {
                    "Key ⬇".to_string()
                } else {
                    t.default_order.to_string()
                }
            }
        };
        ui.add(egui::Label::new(sort_text).sense(egui::Sense::hover()))
            .on_hover_text(t.sort_hint);
        let is_default = !app.sort_desc && app.col_sort.is_none();
        if ui
            .add_enabled(!is_default, egui::Button::new("↺"))
            .on_hover_text(t.reset_order)
            .clicked()
        {
            app.sort_desc = false;
            app.col_sort = None;
            app.load_first_page();
        }

        ui.separator();
        if ui.button("⏮").on_hover_text(t.tip_first).clicked() {
            app.load_first_page();
        }
        if ui
            .add_enabled(!app.at_start, egui::Button::new("◀"))
            .on_hover_text(t.tip_prev)
            .clicked()
        {
            app.load_prev_page();
        }
        if ui
            .add_enabled(!app.at_end, egui::Button::new("▶"))
            .on_hover_text(t.tip_next)
            .clicked()
        {
            app.load_next_page();
        }
        if ui.button("⏭").on_hover_text(t.tip_last).clicked() {
            app.load_last_page();
        }

        ui.separator();
        // Value 搜索模式开关：放在搜索框前（先选模式再输入），
        // 提示与主题切换一样按状态分开（提示将切换到的模式）
        if ui
            .checkbox(&mut app.value_search_full, t.full_search_toggle)
            .on_hover_text(if app.value_search_full {
                t.full_search_tip_on
            } else {
                t.full_search_tip_off
            })
            .changed()
        {
            // 切到全表模式时清除页内过滤残留，避免干扰
            if app.value_search_full {
                app.clear_value_search();
            }
        }

        // Key 搜索模式开关：➡ 跳转定位 / F 前缀过滤（F 与 K/V 按钮风格一致，
        // 避免放大镜图标被误认成"搜索"）。
        // 放在搜索框前（先选模式再输入），提示与主题切换一样按状态分开。
        if ui
            .button(if app.key_filter_mode { "F" } else { "➡" })
            .on_hover_text(if app.key_filter_mode {
                t.key_mode_tip_on
            } else {
                t.key_mode_tip_off
            })
            .clicked()
        {
            app.key_filter_mode = !app.key_filter_mode;
        }

        // 共享搜索框：Key（跳转/前缀过滤）和 Value（全表/页内文本搜索）共用
        // 符号取自 egui 内置字体 emoji-icon/NotoEmoji，跨平台不依赖系统字体
        let sresp = ui.add(
            egui::TextEdit::singleline(&mut app.search_input)
                .id_salt("search_box")
                .desired_width(160.0)
                .hint_text(t.toolbar_search_hint),
        );
        // Ctrl+F 的延迟聚焦：在实际控件上 request_focus（对猜测的 ID 直接
        // request_focus 会因 ID 不存在触发 accesskit panic）
        if app.focus_search {
            sresp.request_focus();
            app.focus_search = false;
        }
        app.search_box_id = Some(sresp.id);
        // 搜索框聚焦 = Ctrl+C 归框内文本复制，解除行复制武装
        if sresp.gained_focus() {
            app.row_copy_pending = false;
        }
        // 回车默认执行 Key 搜索（跳转或前缀过滤，取决于模式）
        if sresp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            app.apply_key_search();
        }
        // 搜索 Key
        if ui.button("K").on_hover_text(t.key_search_btn_tip).clicked() {
            app.apply_key_search();
        }
        // 搜索 Value：行为由"全表"开关决定，提示按当前模式分状态
        let v_btn = ui.button("V").on_hover_text(if app.value_search_full {
            t.value_search_tip_on
        } else {
            t.value_search_tip_off
        });
        if v_btn.clicked() {
            if app.value_search_full {
                app.start_full_value_search();
            } else {
                app.apply_value_search();
            }
        }
        // 过滤状态指示 + 清除
        if app.key_filter.is_some() || app.value_filter.is_some() {
            ui.label(egui::RichText::new(t.filter_active).color(ui.visuals().warn_fg_color));
            if ui.button("×").on_hover_text(t.filter_clear_tip).clicked() {
                app.clear_key_search();
                app.clear_value_search();
            }
        }
    });

    ui.separator();

    // ── 表格 ────────────────────────────────────────────────────
    let text_height = egui::TextStyle::Body.resolve(ui.style()).size + 4.0;
    let order = app.display_order();
    let total_rows = order.len();
    let selected = app.selected_row;
    let sort_desc = app.sort_desc;
    let col_sort = app.col_sort;

    // 列头标题（带排序指示；默认读取顺序时不显示箭头）
    let key_title = if sort_desc { "Key ⬇" } else { "Key" };
    let col_title = |col: SortCol, base: &str| match col_sort {
        Some((c, true)) if c == col => format!("{base} ⬆"),
        Some((c, false)) if c == col => format!("{base} ⬇"),
        _ => base.to_string(),
    };
    let index_title = col_title(SortCol::Index, "#");
    let type_title = col_title(SortCol::Type, &SortCol::Type.label());
    let value_title = col_title(SortCol::Value, "Value");

    // 列头被点击的列：0=# 1=Key 2=类型 3=Value
    let mut col_clicked: Option<u8> = None;
    let mut clicked_row = None;

    // 选中行用浅色底 + 默认文字色（不再反白），
    // 截断 tooltip 等继承颜色的场景就不会再出现白字白底；scope 内生效不外泄
    ui.scope(|ui| {
        let bg = if ui.visuals().dark_mode {
            egui::Color32::from_rgb(0x2F, 0x4A, 0x66)
        } else {
            egui::Color32::from_rgb(0xC8, 0xDC, 0xF0)
        };
        ui.style_mut().visuals.selection.bg_fill = bg;
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(56.0))
            .column(Column::initial(160.0).at_least(80.0).clip(true))
            .column(Column::exact(78.0))
            .column(Column::remainder().clip(true))
            .min_scrolled_height(0.0)
            .header(text_height, |mut header| {
                header.col(|ui| {
                    if header_cell(ui, &index_title, t.col_tip_page) {
                        col_clicked = Some(0);
                    }
                });
                header.col(|ui| {
                    if header_cell(ui, key_title, t.col_tip_key) {
                        col_clicked = Some(1);
                    }
                });
                header.col(|ui| {
                    if header_cell(ui, &type_title, t.col_tip_page) {
                        col_clicked = Some(2);
                    }
                });
                header.col(|ui| {
                    if header_cell(ui, &value_title, t.col_tip_page) {
                        col_clicked = Some(3);
                    }
                });
            })
            .body(|body| {
                body.rows(text_height, total_rows, |mut row_ui| {
                    let di = row_ui.index();
                    let i = order[di];
                    let row = &app.rows[i];
                    let view = &app.views[i];
                    let is_sel = selected == Some(i);
                    row_ui.set_selected(is_sel);
                    // 文字保持默认色：选中行底色已改浅色，无需反白
                    row_ui.col(|ui| {
                        let abs = match app.base_index {
                            Some(b) => b + i + 1,
                            None => i + 1,
                        };
                        let rt = egui::RichText::new(abs.to_string());
                        // selectable(false)：避免单元格文本选区抢占 Ctrl+C，
                        // 保证 Ctrl+C 始终复制选中行的 KV
                        let resp = ui.add(
                            egui::Label::new(rt)
                                .selectable(false)
                                .show_tooltip_when_elided(false)
                                .sense(egui::Sense::click()),
                        );
                        elide_tooltip(ui, resp.clone(), &abs.to_string());
                        if resp.clicked() {
                            clicked_row = Some(i);
                            app.row_copy_pending = true;
                        }
                    });
                    row_ui.col(|ui| {
                        let rt = egui::RichText::new(&view.key_text).monospace();
                        let resp = ui.add(
                            egui::Label::new(rt)
                                .selectable(false)
                                .show_tooltip_when_elided(false)
                                .sense(egui::Sense::click()),
                        );
                        elide_tooltip(ui, resp.clone(), &view.key_text);
                        if resp.clicked() {
                            clicked_row = Some(i);
                            app.row_copy_pending = true;
                        }
                    });
                    row_ui.col(|ui| {
                        let rt = egui::RichText::new(&view.type_label);
                        let resp = ui.add(
                            egui::Label::new(rt)
                                .selectable(false)
                                .show_tooltip_when_elided(false)
                                .sense(egui::Sense::click()),
                        );
                        elide_tooltip(ui, resp.clone(), &view.type_label);
                        if resp.clicked() {
                            clicked_row = Some(i);
                            app.row_copy_pending = true;
                        }
                    });
                    row_ui.col(|ui| {
                        // 多值表分组行：显示第一个值的预览 + 值总数（值列表在右侧翻看）
                        let text = match row.dup_count {
                            Some(n) => format!("{}{}", view.val_text, t.dup_n_values(n)),
                            None => view.val_text.clone(),
                        };
                        let rt = egui::RichText::new(&text).monospace();
                        let resp = ui.add(
                            egui::Label::new(rt)
                                .selectable(false)
                                .show_tooltip_when_elided(false)
                                .sense(egui::Sense::click()),
                        );
                        elide_tooltip(ui, resp.clone(), &text);
                        if resp.clicked() {
                            clicked_row = Some(i);
                            app.row_copy_pending = true;
                        }
                    });
                    if row_ui.response().clicked() {
                        clicked_row = Some(i);
                    }
                });
            });
    });

    // ── 列头排序点击处理 ─────────────────────────────────────────
    match col_clicked {
        Some(1) => {
            // Key 列：切换全局遍历方向（翻页保持）
            app.sort_desc = !app.sort_desc;
            app.col_sort = None;
            app.load_first_page();
        }
        Some(c) => {
            // 其余列：页内排序，同一列循环 升序 → 降序 → 恢复默认
            let col = match c {
                0 => SortCol::Index,
                2 => SortCol::Type,
                _ => SortCol::Value,
            };
            app.col_sort = match app.col_sort {
                Some((cc, true)) if cc == col => Some((col, false)),
                Some((cc, false)) if cc == col => None,
                _ => Some((col, true)),
            };
        }
        None => {}
    }
    if let Some(i) = clicked_row {
        app.select_row(i);
    }
}
