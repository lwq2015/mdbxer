//! 中间 "数据" 页签：工具条 + 表格。

use super::{MdbxerApp, PAGE_SIZES, SortCol};
use crate::fmt;
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

pub fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let Some(table) = app.cur_table().cloned() else {
        ui.label("请选择左侧表");
        return;
    };

    // ── 工具条 ──────────────────────────────────────────────────
    ui.horizontal(|ui| {
        // 默认顺序 = 表中读取出来的顺序（正向遍历）
        let sort_text = match app.col_sort {
            Some((col, asc)) => {
                format!("页内排序：{} {}", col.label(), if asc { "↑" } else { "↓" })
            }
            None => {
                if app.sort_desc {
                    "Key 反向 ↓".to_string()
                } else {
                    "读取顺序（默认）".to_string()
                }
            }
        };
        ui.label(sort_text);
        let is_default = !app.sort_desc && app.col_sort.is_none();
        if ui
            .add_enabled(!is_default, egui::Button::new("↺ 默认顺序"))
            .on_hover_text("恢复为表中读取出来的顺序")
            .clicked()
        {
            app.sort_desc = false;
            app.col_sort = None;
            app.load_first_page();
        }

        ui.separator();
        if ui.button("⏮ 首条").clicked() {
            app.load_first_page();
        }
        if ui.add_enabled(!app.at_start, egui::Button::new("◀ 上一页")).clicked() {
            app.load_prev_page();
        }
        if ui.add_enabled(!app.at_end, egui::Button::new("下一页 ▶")).clicked() {
            app.load_next_page();
        }
        if ui.button("末条 ⏭").clicked() {
            app.load_last_page();
        }

        ui.separator();
        let resp = ui.add(
            egui::TextEdit::singleline(&mut app.jump_input)
                .desired_width(160.0)
                .hint_text("hex(...) 或纯文本跳转到 Key"),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            app.jump();
        }
        if ui.button("跳转").clicked() {
            app.jump();
        }

        ui.separator();
        let mut ps = app.page_size;
        let ir = egui::ComboBox::from_id_salt("page_size")
            .selected_text(format!("{ps}"))
            .show_ui(ui, |ui| {
                for &v in &PAGE_SIZES {
                    ui.selectable_value(&mut ps, v, v.to_string());
                }
            });
        super::wheel_cycle(ui.ctx(), &ir.response, &PAGE_SIZES, &mut ps);
        if ps != app.page_size {
            app.page_size = ps;
            app.load_first_page();
        }

        ui.separator();
        let (start, end) = if app.rows.is_empty() {
            (0, 0)
        } else {
            match app.base_index {
                Some(b) => (b + 1, b + app.rows.len()),
                None => (1, app.rows.len()),
            }
        };
        ui.label(format!("第 {start}~{end} 条 / 共 {} 条", table.entries));
    });

    ui.separator();

    // ── 表格 ────────────────────────────────────────────────────
    let text_height = egui::TextStyle::Body.resolve(ui.style()).size + 4.0;
    let total_rows = app.rows.len();
    let selected = app.selected_row;
    let cell_max = app.cell_max;
    let grid_mode = app.grid_mode;
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
    let type_title = col_title(SortCol::Type, "类型");
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
                    let key_text = fmt::decode(&row.key, grid_mode, cell_max);
                    let mut rt = egui::RichText::new(key_text).monospace();
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
                    let (label, _) = fmt::guess(&row.value);
                    let mut rt = egui::RichText::new(label);
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
                    let val_text = fmt::decode(&row.value, grid_mode, cell_max);
                    let mut rt = egui::RichText::new(val_text).monospace();
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
