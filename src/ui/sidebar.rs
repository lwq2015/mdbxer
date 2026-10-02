// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 左侧表（subDB）列表：过滤、排序、条数显示。

use super::{LEFT_PANEL_MAX, LEFT_PANEL_MIN, MIDDLE_MIN_WIDTH, MdbxerApp, TableSort};
use crate::i18n::tr;

/// 左侧表列表面板：标题 + 排序下拉 + 过滤框 + 可滚动列表。
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
                app.table_sort = sort;
            });
            ui.add(
                egui::TextEdit::singleline(&mut app.table_filter)
                    .hint_text(t.filter_hint)
                    .desired_width(f32::INFINITY),
            );
            ui.separator();

            let Some(dbh) = &app.db else { return };

            // 过滤 + 排序后的索引
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
                TableSort::CountAsc => {
                    idx.sort_by_key(|&i| dbh.tables[i].entries)
                }
                TableSort::CountDesc => {
                    idx.sort_by_key(|&i| std::cmp::Reverse(dbh.tables[i].entries))
                }
            }

            let mut clicked = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for i in idx {
                    let tbl = &dbh.tables[i];
                    let selected = app.selected_table == Some(i);
                    let text = t.table_entry(&tbl.display(), tbl.entries, &tbl.flags_desc());
                    if ui
                        .selectable_label(selected, egui::RichText::new(text).monospace())
                        .clicked()
                    {
                        clicked = Some(i);
                    }
                }
            });
            if let Some(i) = clicked {
                app.select_table(i);
            }
        });
    app.left_panel_w = resp.response.rect.width();
}
