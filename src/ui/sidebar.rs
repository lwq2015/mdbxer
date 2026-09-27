//! 左侧表（subDB）列表：过滤、排序、条数显示。

use super::{MdbxerApp, TableSort};

pub fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    egui::Panel::left("table_list")
        .default_size(220.0)
        .resizable(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("表 (subDB)");
                ui.separator();
                let mut sort = app.table_sort;
                egui::ComboBox::from_id_salt("table_sort")
                    .selected_text(sort.label())
                    .show_ui(ui, |ui| {
                        for s in TableSort::ALL {
                            ui.selectable_value(&mut sort, s, s.label());
                        }
                    });
                app.table_sort = sort;
            });
            ui.add(
                egui::TextEdit::singleline(&mut app.table_filter)
                    .hint_text("过滤表名")
                    .desired_width(f32::INFINITY),
            );
            ui.separator();

            let Some(dbh) = &app.db else { return };

            // 过滤 + 排序后的索引
            let filter = app.table_filter.to_lowercase();
            let mut idx: Vec<usize> = (0..dbh.tables.len()).collect();
            idx.retain(|&i| {
                filter.is_empty() || dbh.tables[i].display.to_lowercase().contains(&filter)
            });
            match app.table_sort {
                TableSort::NameAsc => idx.sort_by(|&a, &b| {
                    dbh.tables[a].display.cmp(&dbh.tables[b].display)
                }),
                TableSort::NameDesc => idx.sort_by(|&a, &b| {
                    dbh.tables[b].display.cmp(&dbh.tables[a].display)
                }),
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
                    let t = &dbh.tables[i];
                    let selected = app.selected_table == Some(i);
                    let mut text = format!("{}  ({} 条", t.display, t.entries);
                    if !t.flags_desc.is_empty() {
                        text.push_str(&format!("，{}", t.flags_desc));
                    }
                    text.push(')');
                    if ui.selectable_label(selected, text).clicked() {
                        clicked = Some(i);
                    }
                }
            });
            if let Some(i) = clicked {
                app.select_table(i);
            }
        });
}
