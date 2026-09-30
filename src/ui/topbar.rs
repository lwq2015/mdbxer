//! 顶栏：路径/打开/历史/模式 + 字节序/排版/单元格/面板开关（单行紧凑布局）。

use super::MdbxerApp;
use crate::db::OpenMode;
use crate::fmt::{DecodeMode, Endian};

pub fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    egui::Panel::top("top_bar").show(ui, |ui| {
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut app.path_input)
                    .desired_width(240.0)
                    .hint_text("数据库目录或 .mdbx 文件（可拖入窗口）"),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                app.open_db();
            }
            if ui.button("打开").clicked() {
                app.open_db();
            }
            if app.db.is_some() && ui.button("关闭").clicked() {
                app.close_db();
            }
            if ui.button("文件").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("MDBX", &["mdbx", "dat", "*"])
                    .pick_file()
                {
                    app.path_input = p.display().to_string();
                    app.open_mode = OpenMode::SingleFile;
                    app.open_db();
                }
            }
            if ui.button("目录").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    app.path_input = p.display().to_string();
                    app.open_mode = OpenMode::Directory;
                    app.open_db();
                }
            }

            let mut mode = app.open_mode;
            let ir = egui::ComboBox::from_id_salt("open_mode")
                .width(70.0)
                .selected_text(mode.label())
                .show_ui(ui, |ui| {
                    for m in OpenMode::ALL {
                        ui.selectable_value(&mut mode, m, m.label());
                    }
                });
            super::wheel_cycle(ui.ctx(), &ir.response, &OpenMode::ALL, &mut mode);
            app.open_mode = mode;

            // 历史记录
            let mut pick = None;
            let mut del = None;
            egui::ComboBox::from_id_salt("history")
                .width(56.0)
                .selected_text("历史")
                .show_ui(ui, |ui| {
                    if app.history.entries.is_empty() {
                        ui.label("（暂无历史记录）");
                    }
                    for (i, e) in app.history.entries.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.selectable_label(false, &e.path).clicked() {
                                pick = Some(i);
                            }
                            if ui.small_button("✕").clicked() {
                                del = Some(i);
                            }
                        });
                    }
                });
            if let Some(i) = pick {
                let e = app.history.entries[i].clone();
                app.path_input = e.path;
                app.open_mode = OpenMode::from_str(&e.mode);
                app.open_db();
            }
            if let Some(i) = del {
                app.history.remove(i);
            }

            ui.separator();

            // 字节序（默认小端，可切大端；影响整数/float/double 与自动猜测）
            ui.label("字节序");
            let mut en = app.endian;
            let ir = egui::ComboBox::from_id_salt("endian")
                .width(54.0)
                .selected_text(en.suffix())
                .show_ui(ui, |ui| {
                    for e in Endian::ALL {
                        ui.selectable_value(&mut en, e, e.label());
                    }
                });
            super::wheel_cycle(ui.ctx(), &ir.response, &Endian::ALL, &mut en);
            app.endian = en;

            // 排版（数据页 Key/Value 列的显示格式）
            ui.label("排版");
            let mut gm = app.grid_mode;
            let ir = egui::ComboBox::from_id_salt("grid_mode")
                .width(92.0)
                .selected_text(gm.label())
                .height(430.0)
                .show_ui(ui, |ui| {
                    for m in DecodeMode::ALL {
                        ui.selectable_value(&mut gm, m, m.label());
                    }
                });
            super::wheel_cycle(ui.ctx(), &ir.response, &DecodeMode::ALL, &mut gm);
            app.grid_mode = gm;

            let mut cm = app.cell_max;
            let ir = egui::ComboBox::from_id_salt("cell_max")
                .width(60.0)
                .selected_text(format!("{cm}"))
                .show_ui(ui, |ui| {
                    for v in [64usize, 128, 256, 512, 1024, 4096] {
                        ui.selectable_value(&mut cm, v, v.to_string());
                    }
                });
            let cm_resp = ir
                .response
                .on_hover_text("单元格最多显示的字符数（超出截断）");
            super::wheel_cycle(
                ui.ctx(),
                &cm_resp,
                &[64usize, 128, 256, 512, 1024, 4096],
                &mut cm,
            );
            app.cell_max = cm;

            ui.separator();

            let mut lv = app.left_visible;
            if ui.selectable_label(lv, "表").clicked() {
                lv = !lv;
            }
            app.left_visible = lv;
            let mut dv = app.detail_visible;
            if ui.selectable_label(dv, "详情").clicked() {
                dv = !dv;
            }
            app.detail_visible = dv;
        });
    });
}
