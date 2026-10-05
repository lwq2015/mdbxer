// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 顶栏：打开模式/历史 + 文件/目录按钮 + 字节序/排版/单元格/面板开关（单行紧凑布局）。
//! 最右侧为语言下拉。

use super::MdbxerApp;
use crate::config;
use crate::db::OpenMode;
use crate::fmt::{DecodeMode, Endian};
use crate::i18n::{self, Lang};

/// 顶栏面板：打开模式/历史 + 文件/目录按钮 + 字节序/排版/单元格 + 表/详情开关 + 语言。
pub fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let t = i18n::tr().clone();
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

            if ui.button(t.btn_file).clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("MDBX", &["mdbx", "dat", "*"])
                    .pick_file()
                {
                    app.open_mode = OpenMode::SingleFile;
                    app.open_db(&p.display().to_string());
                }
            }
            if ui.button(t.btn_dir).clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    app.open_mode = OpenMode::Directory;
                    app.open_db(&p.display().to_string());
                }
            }
            if app.db.is_some() && ui.button(t.btn_close).clicked() {
                app.close_db();
            }

            // 历史记录
            let mut pick = None;
            let mut del = None;
            egui::ComboBox::from_id_salt("history")
                .width(56.0)
                .selected_text(t.history)
                .show_ui(ui, |ui| {
                    if app.history.entries.is_empty() {
                        ui.label(t.history_empty);
                    }
                    for (i, e) in app.history.entries.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.selectable_label(false, &e.path).clicked() {
                                pick = Some(i);
                            }
                            if ui
                                .small_button("×")
                                .on_hover_text(t.history_del_tip)
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
            ui.label(t.endian);
            let mut en = app.endian;
            let ir = egui::ComboBox::from_id_salt("endian")
                .width(72.0)
                .selected_text(en.suffix())
                .show_ui(ui, |ui| {
                    for e in Endian::ALL {
                        ui.selectable_value(&mut en, e, e.label());
                    }
                });
            super::wheel_cycle(ui.ctx(), &ir.response, &Endian::ALL, &mut en);
            if app.endian != en {
                app.endian = en;
                app.save_ui_prefs();
            }

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
            if app.key_mode != km {
                app.key_mode = km;
                app.save_ui_prefs();
            }

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
            if app.val_mode != vm {
                app.val_mode = vm;
                app.save_ui_prefs();
            }

            let mut cm = app.cell_max;
            let ir = egui::ComboBox::from_id_salt("cell_max")
                .width(60.0)
                .selected_text(format!("{cm}"))
                .show_ui(ui, |ui| {
                    for v in [64usize, 128, 256, 512, 1024, 4096] {
                        ui.selectable_value(&mut cm, v, v.to_string());
                    }
                });
            let cm_resp = ir.response.on_hover_text(t.cell_max_tip);
            super::wheel_cycle(
                ui.ctx(),
                &cm_resp,
                &[64usize, 128, 256, 512, 1024, 4096],
                &mut cm,
            );
            if app.cell_max != cm {
                app.cell_max = cm;
                app.save_ui_prefs();
            }

            // 整数千位分隔开关（影响所有 decode/guess 输出）
            let mut ts = crate::fmt::thousands_sep();
            if ui
                .selectable_label(ts, "1,234")
                .on_hover_text(t.thousands_tip)
                .clicked()
            {
                ts = !ts;
                crate::fmt::set_thousands_sep(ts);
                app.save_ui_prefs();
            }

            ui.separator();

            let mut lv = app.left_visible;
            if ui.selectable_label(lv, t.panel_tables).clicked() {
                lv = !lv;
            }
            app.left_visible = lv;
            let mut dv = app.detail_visible;
            if ui.selectable_label(dv, t.panel_detail).clicked() {
                dv = !dv;
            }
            app.detail_visible = dv;

            // 语言选择：顶栏最右侧，切换即时生效并持久化
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let old_lang = i18n::lang();
                let mut lang = old_lang;
                egui::ComboBox::from_id_salt("lang")
                    .width(86.0)
                    .selected_text(lang.label())
                    .show_ui(ui, |ui| {
                        for l in Lang::ALL {
                            ui.selectable_value(&mut lang, l, l.label());
                        }
                    });
                ui.label(t.language);
                if lang != old_lang {
                    i18n::set_lang(lang);
                    config::save_lang(lang);
                    // 统计/环境页是打开时缓存的，语言切换后强制重读
                    app.stat_cache = None;
                    app.env_cache = None;
                }
                if ui
                    .button(if app.dark_theme { "🌙" } else { "☀" })
                    .on_hover_text(t.theme_tip)
                    .clicked()
                {
                    app.toggle_theme(ui.ctx());
                }
            });
        });
    });
}
