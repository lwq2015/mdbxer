// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 中间 "数据" 页签：工具条 + 表格。

use super::{MdbxerApp, PAGE_SIZES, SortCol};
use crate::i18n::tr;
use egui_extras::{Column, TableBuilder};

/// 列头单元格：文字与整列空白都可点击。返回是否被点击。
fn header_cell(ui: &mut egui::Ui, text: &str) -> bool {
    let r1 = ui.add(
        egui::Label::new(egui::RichText::new(text).strong()).sense(egui::Sense::click()),
    );
    // 覆盖列内剩余空白区域，使整列都可点击
    let r2 = ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::click());
    r1.clicked() || r2.clicked()
}

/// "数据"页签入口：工具条 + 行表格 + 列头排序处理。
pub fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let t = tr();
    if app.cur_table().is_none() {
        ui.label(t.select_table);
        return;
    }

    // ── 工具条（单行紧凑：符号按钮 + 悬停说明；范围信息见底部状态栏）──
    ui.horizontal(|ui| {
        // 默认顺序 = 表中读取出来的顺序（正向遍历）
        let sort_text = match app.col_sort {
            Some((col, asc)) => {
                format!("{} {}", col.label(), if asc { "↑" } else { "↓" })
            }
            None => {
                if app.sort_desc {
                    "Key ↓".to_string()
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
        // Key 搜索：回车生效；→ 跳转定位 / ⊂ 前缀过滤
        let sresp = ui.add(
            egui::TextEdit::singleline(&mut app.key_search_input)
                .desired_width(140.0)
                .hint_text(t.key_search_hint),
        );
        if sresp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            app.apply_key_search();
        }
        if ui
            .button(if app.key_filter_mode { "⊂" } else { "→" })
            .on_hover_text(t.key_search_mode_tip)
            .clicked()
        {
            app.key_filter_mode = !app.key_filter_mode;
        }
        if app.key_filter.is_some() {
            ui.label(
                egui::RichText::new(t.filter_active).color(ui.visuals().warn_fg_color),
            );
            if ui
                .button("×")
                .on_hover_text(t.filter_clear_tip)
                .clicked()
            {
                app.clear_key_search();
            }
        }

        ui.separator();
        let resp = ui.add(
            egui::TextEdit::singleline(&mut app.jump_input)
                .desired_width(88.0)
                .hint_text(t.jump_hint),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            app.jump();
        }
        if ui.button(t.jump_btn).on_hover_text(t.jump_btn).clicked() {
            app.jump();
        }

        ui.separator();
        let mut ps = app.page_size;
        let ir = egui::ComboBox::from_id_salt("page_size")
            .width(60.0)
            .selected_text(format!("{ps}"))
            .show_ui(ui, |ui| {
                for &v in &PAGE_SIZES {
                    ui.selectable_value(&mut ps, v, v.to_string());
                }
            });
        let page_resp = ir.response.on_hover_text(t.page_size_tip);
        super::wheel_cycle(ui.ctx(), &page_resp, &PAGE_SIZES, &mut ps);
        if ps != app.page_size {
            app.page_size = ps;
            app.save_ui_prefs();
            app.load_first_page();
        }

        ui.separator();
        // 导出当前表：CSV/JSON 下拉 + ↓（导出中禁用防重入）
        // 符号用 U+2193（雅黑/egui 内置字体均含）；U+21E9(⇩) 在雅黑中无字形会变豆腐块
        let mut ef = app.export_format;
        egui::ComboBox::from_id_salt("export_format")
            .width(60.0)
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
            .add_enabled(app.export_ev_rx.is_none(), egui::Button::new("↓"))
            .on_hover_text(t.export_tip)
            .clicked()
        {
            app.start_export();
        }
    });

    ui.separator();

    // ── 表格 ────────────────────────────────────────────────────
    let text_height = egui::TextStyle::Body.resolve(ui.style()).size + 4.0;
    let total_rows = app.rows.len();
    let selected = app.selected_row;
    let order = app.display_order();
    let sort_desc = app.sort_desc;
    let col_sort = app.col_sort;

    // 列头标题（带排序指示；默认读取顺序时不显示箭头）
    let key_title = if sort_desc { "Key ↓" } else { "Key" };
    let col_title = |col: SortCol, base: &str| match col_sort {
        Some((c, true)) if c == col => format!("{base} ↑"),
        Some((c, false)) if c == col => format!("{base} ↓"),
        _ => base.to_string(),
    };
    let index_title = col_title(SortCol::Index, "#");
    let type_title = col_title(SortCol::Type, &SortCol::Type.label());
    let value_title = col_title(SortCol::Value, "Value");

    // 列头被点击的列：0=# 1=Key 2=类型 3=Value
    let mut col_clicked: Option<u8> = None;
    let mut clicked_row = None;

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(56.0))
        .column(Column::initial(240.0).at_least(80.0).clip(true))
        .column(Column::exact(80.0))
        .column(Column::remainder().clip(true))
        .min_scrolled_height(0.0)
        .header(text_height, |mut header| {
            header.col(|ui| {
                if header_cell(ui, &index_title) {
                    col_clicked = Some(0);
                }
            });
            header.col(|ui| {
                if header_cell(ui, key_title) {
                    col_clicked = Some(1);
                }
            });
            header.col(|ui| {
                if header_cell(ui, &type_title) {
                    col_clicked = Some(2);
                }
            });
            header.col(|ui| {
                if header_cell(ui, &value_title) {
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
                // 选中行文字反白，与文本选区颜色区分开
                let sel_color = if is_sel {
                    Some(egui::Color32::WHITE)
                } else {
                    None
                };
                row_ui.col(|ui| {
                    let abs = match app.base_index {
                        Some(b) => b + i + 1,
                        None => i + 1,
                    };
                    let mut rt = egui::RichText::new(abs.to_string());
                    if let Some(c) = sel_color {
                        rt = rt.color(c);
                    }
                    if ui
                        .add(egui::Label::new(rt).sense(egui::Sense::click()))
                        .clicked()
                    {
                        clicked_row = Some(i);
                    }
                });
                row_ui.col(|ui| {
                    let mut rt = egui::RichText::new(&view.key_text).monospace();
                    if let Some(c) = sel_color {
                        rt = rt.color(c);
                    }
                    if ui
                        .add(egui::Label::new(rt).sense(egui::Sense::click()))
                        .clicked()
                    {
                        clicked_row = Some(i);
                    }
                });
                row_ui.col(|ui| {
                    let mut rt = egui::RichText::new(&view.type_label);
                    if let Some(c) = sel_color {
                        rt = rt.color(c);
                    }
                    if ui
                        .add(egui::Label::new(rt).sense(egui::Sense::click()))
                        .clicked()
                    {
                        clicked_row = Some(i);
                    }
                });
                row_ui.col(|ui| {
                    // 多值表分组行：显示第一个值的预览 + 值总数（值列表在右侧翻看）
                    let text = match row.dup_count {
                        Some(n) => format!("{}{}", view.val_text, t.dup_n_values(n)),
                        None => view.val_text.clone(),
                    };
                    let mut rt = egui::RichText::new(text).monospace();
                    if let Some(c) = sel_color {
                        rt = rt.color(c);
                    }
                    if ui
                        .add(egui::Label::new(rt).sense(egui::Sense::click()))
                        .on_hover_text(t.dup_row_tip)
                        .clicked()
                    {
                        clicked_row = Some(i);
                    }
                });
                if row_ui.response().clicked() {
                    clicked_row = Some(i);
                }
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
