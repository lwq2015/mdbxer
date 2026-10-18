// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 顶栏：打开模式/历史 + 文件/目录按钮 + 排版/面板开关（单行紧凑布局）。
//! 字节序/单元格长度/每页条数/千位分隔等不常用开关已移至「高级配置」页。
//! 最右侧为语言下拉。

use super::MdbxerApp;
use crate::config;
use crate::db::OpenMode;
use crate::fmt::DecodeMode;
use crate::i18n::{self, Lang};

/// 在系统文件管理器中打开库所在位置（平台相关）：单文件模式定位文件本身
/// （选中高亮），目录模式直接打开该目录。打开失败静默忽略。
fn reveal_in_folder(path: &str, is_single_file: bool) {
    #[cfg(target_os = "windows")]
    {
        let mut c = std::process::Command::new("explorer");
        if is_single_file {
            c.arg("/select,").arg(path);
        } else {
            c.arg(path);
        }
        let _ = c.spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = std::process::Command::new("open");
        if is_single_file {
            // -R：在 Finder 中定位并高亮文件
            c.arg("-R").arg(path);
        } else {
            c.arg(path);
        }
        let _ = c.spawn();
    }
    #[cfg(target_os = "linux")]
    {
        // Linux 无统一"定位文件"，仅打开所在目录
        let dir = if is_single_file {
            std::path::Path::new(path)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string())
        } else {
            path.to_string()
        };
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

/// 顶栏面板：文件/目录/关闭 + 历史 + Key/Value 排版 + 导出 + 表/详情开关 + 语言。
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
            // 重新加载库：仅重枚举表列表/条目数（复用环境句柄，不重开环境）
            if app.db.is_some()
                && ui
                    .button("↻")
                    .on_hover_text(t.btn_reload_tip)
                    .clicked()
            {
                app.reload_db();
            }
            // 打开库所在位置：单文件模式定位文件，目录模式打开目录
            if let (Some(h), Some(p)) = (&app.db, &app.opened_path) {
                if ui.button("📂").on_hover_text(t.btn_reveal_tip).clicked() {
                    reveal_in_folder(p, h.no_sub_dir);
                }
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

            // Key 排版（默认自动；编码固定的表可手动指定）
            ui.label("Key");
            let mut km = app.key_mode;
            let flat = DecodeMode::flat_all();
            let ir = egui::ComboBox::from_id_salt("key_mode")
                .width(82.0)
                .selected_text(km.label())
                .height(430.0)
                .show_ui(ui, |ui| {
                    // 自动项置顶；其余按"文本编码/数值/字节视图/标识与时间/结构化"分组
                    ui.selectable_value(&mut km, DecodeMode::Auto, DecodeMode::Auto.label());
                    for (grp, items) in super::format_groups() {
                        // 组标题：弱化小字，不可选中
                        ui.add(egui::Label::new(egui::RichText::new(grp).weak().small()));
                        for m in items {
                            ui.selectable_value(&mut km, *m, m.label());
                        }
                    }
                });
            super::wheel_cycle(ui.ctx(), &ir.response, &flat, &mut km);
            if app.key_mode != km {
                // 顶栏排版只管表格列；右栏详情卡片有独立排版，不联动。
                app.key_mode = km;
                app.save_ui_prefs();
            }

            // Value 排版（默认自动：Value 逐行猜测）
            ui.label("Value");
            let mut vm = app.val_mode;
            let flat = DecodeMode::flat_all();
            let ir = egui::ComboBox::from_id_salt("val_mode")
                .width(82.0)
                .selected_text(vm.label())
                .height(430.0)
                .show_ui(ui, |ui| {
                    // 自动项置顶；其余按"文本编码/数值/字节视图/标识与时间/结构化"分组
                    ui.selectable_value(&mut vm, DecodeMode::Auto, DecodeMode::Auto.label());
                    for (grp, items) in super::format_groups() {
                        // 组标题：弱化小字，不可选中
                        ui.add(egui::Label::new(egui::RichText::new(grp).weak().small()));
                        for m in items {
                            ui.selectable_value(&mut vm, *m, m.label());
                        }
                    }
                });
            super::wheel_cycle(ui.ctx(), &ir.response, &flat, &mut vm);
            if app.val_mode != vm {
                app.val_mode = vm;
                app.save_ui_prefs();
            }

            // 导出按钮 + 格式下拉（从左栏迁移过来，更紧凑、不占左栏空间）
            let mut ef = app.export_format;
            let ef_resp = egui::ComboBox::from_id_salt("export_format")
                .width(52.0)
                .selected_text(ef.label())
                .show_ui(ui, |ui| {
                    for f in crate::export::ExportFormat::ALL {
                        ui.selectable_value(&mut ef, f, f.label());
                    }
                })
                .response;
            let _ = ef_resp.on_hover_text(t.export_tip);
            if ui
                .add_enabled(
                    app.db.is_some() && app.export_ev_rx.is_none(),
                    egui::Button::new("⬇"),
                )
                .on_hover_text(t.export_tip)
                .clicked()
            {
                app.start_export();
            }
            if ef != app.export_format {
                app.export_format = ef;
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
