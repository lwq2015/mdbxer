// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 顶栏：打开模式/历史 + 文件/目录按钮 + 字节序/排版/单元格/面板开关（单行紧凑布局）。
//! 最右侧为语言下拉。

use super::MdbxerApp;
use crate::config;
use crate::db::OpenMode;
use crate::fmt::{DecodeMode, Endian};
use crate::i18n::{self, Lang};

/// 顶栏面板：文件/目录/关闭 + 历史 + 字节序/排版/单元格 + 表/详情开关 + 语言。
/// 打开模式不显示选择器：由各打开入口（文件钮=单文件、目录钮=目录、历史=记录值、拖入=自动）自行确定。
pub fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let t = i18n::tr().clone();
    egui::Panel::top("top_bar").show(ui, |ui| {
        ui.horizontal(|ui| {
            // 打开模式不在界面上选择：文件/目录按钮、历史记录、拖入、Ctrl+O
            // 各入口会自行设置 open_mode（自动/单文件/目录），无需用户手动指定。
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
                // 顶栏排版只管表格列；右栏详情卡片有独立排版，不联动。
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

            // 每页显示条数（原数据页工具条，移入顶栏统一管理）
            ui.label(t.page_size_label);
            let mut ps = app.page_size;
            let ir = egui::ComboBox::from_id_salt("page_size")
                .width(60.0)
                .selected_text(format!("{ps}"))
                .show_ui(ui, |ui| {
                    for v in super::PAGE_SIZES {
                        ui.selectable_value(&mut ps, v, v.to_string());
                    }
                });
            let ps_resp = ir.response.on_hover_text(t.page_size_tip);
            super::wheel_cycle(ui.ctx(), &ps_resp, &super::PAGE_SIZES, &mut ps);
            if ps != app.page_size {
                app.page_size = ps;
                app.save_ui_prefs();
                app.load_first_page();
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
            let mut dv = app.detail_visible;
            if ui.selectable_label(dv, t.panel_detail).clicked() {
                dv = !dv;
            }
            if lv != app.left_visible || dv != app.detail_visible {
                app.left_visible = lv;
                app.detail_visible = dv;
                app.save_ui_prefs();
            }

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
                if lang != old_lang {
                    i18n::set_lang(lang);
                    config::save_lang(lang);
                    // 统计/环境页是打开时缓存的，语言切换后强制重读
                    app.stat_cache = None;
                    app.env_cache = None;
                }
                if ui
                    .button(if app.dark_theme { "🌙" } else { "☀" })
                    .on_hover_text(if app.dark_theme {
                        t.theme_to_light
                    } else {
                        t.theme_to_dark
                    })
                    .clicked()
                {
                    app.toggle_theme(ui.ctx());
                }
            });
        });
    });
}
