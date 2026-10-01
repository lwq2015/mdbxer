//! 顶栏：打开模式/历史 + 文件/目录按钮 + 字节序/排版/单元格/面板开关（单行紧凑布局）。

use super::MdbxerApp;
use crate::db::OpenMode;
use crate::fmt::{DecodeMode, Endian};

/// 顶栏面板：打开模式/历史 + 文件/目录按钮 + 字节序/排版/单元格 + 表/详情开关。
pub fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    egui::Panel::top("top_bar").show(ui, |ui| {
        ui.horizontal(|ui| {
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

            if ui.button("文件").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("MDBX", &["mdbx", "dat", "*"])
                    .pick_file()
                {
                    app.open_mode = OpenMode::SingleFile;
                    app.open_db(&p.display().to_string());
                }
            }
            if ui.button("目录").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    app.open_mode = OpenMode::Directory;
                    app.open_db(&p.display().to_string());
                }
            }
            if app.db.is_some() && ui.button("关闭").clicked() {
                app.close_db();
            }

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
                            if ui
                                .small_button("×")
                                .on_hover_text("从历史记录中删除该条")
                                .clicked()
                            {
                                del = Some(i);
                            }
                        });
                    }
                });
            if let Some(i) = pick {
                let e = app.history.entries[i].clone();
                app.open_mode = OpenMode::from_str(&e.mode);
                app.open_db(&e.path);
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

            // Key 排版（默认自动；编码固定的表可手动指定）
            ui.label("Key");
            let mut km = app.key_mode;
            let ir = egui::ComboBox::from_id_salt("key_mode")
                .width(82.0)
                .selected_text(km.label())
                .height(430.0)
                .show_ui(ui, |ui| {
                    for m in DecodeMode::ALL {
                        ui.selectable_value(&mut km, m, m.label());
                    }
                });
            super::wheel_cycle(ui.ctx(), &ir.response, &DecodeMode::ALL, &mut km);
            app.key_mode = km;

            // Value 排版（默认自动：Value 逐行猜测）
            ui.label("Value");
            let mut vm = app.val_mode;
            let ir = egui::ComboBox::from_id_salt("val_mode")
                .width(82.0)
                .selected_text(vm.label())
                .height(430.0)
                .show_ui(ui, |ui| {
                    for m in DecodeMode::ALL {
                        ui.selectable_value(&mut vm, m, m.label());
                    }
                });
            super::wheel_cycle(ui.ctx(), &ir.response, &DecodeMode::ALL, &mut vm);
            app.val_mode = vm;

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

            // 整数千位分隔开关（影响所有 decode/guess 输出）
            let mut ts = crate::fmt::thousands_sep();
            if ui
                .selectable_label(ts, "1,234")
                .on_hover_text("整数千位分隔（仅显示，不影响数据）")
                .clicked()
            {
                ts = !ts;
            }
            crate::fmt::set_thousands_sep(ts);

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
