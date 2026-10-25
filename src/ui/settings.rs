// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 高级配置页（中央页签）：单元格附注开关表 + 从顶栏移入的不常用显示参数。
//! 各开关改动即时生效并持久化（save_ui_prefs）；附注开关变化会经 views_key
//! 失效自动重建当前页显示缓存。

use super::MdbxerApp;

/// 高级配置页主体。
pub(crate) fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(8.0);

        let left_width = left_column_width(ui);
        let shortcuts_width = shortcuts_width(ui);
        let column_gap = 8.0;
        if ui.available_width() >= left_width + column_gap + shortcuts_width {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    hint_section(ui, app, left_width);
                    ui.add_space(10.0);
                    display_section(ui, app, left_width);
                    ui.add_space(10.0);
                    log_section(ui, left_width);
                });
                ui.add_space(column_gap);
                shortcuts_section(ui, shortcuts_width);
            });
        } else {
            hint_section(ui, app, left_width);
            ui.add_space(10.0);
            display_section(ui, app, left_width);
            ui.add_space(10.0);
            log_section(ui, left_width);
            ui.add_space(10.0);
            shortcuts_section(ui, shortcuts_width);
        }
    });
}

fn measure_text(ui: &egui::Ui, text: &str) -> f32 {
    let font_id = egui::TextStyle::Body.resolve(ui.style());
    ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(text.to_owned(), font_id, egui::Color32::WHITE)
            .size()
            .x
    })
}

fn max_text_width(ui: &egui::Ui, texts: &[&str]) -> f32 {
    texts
        .iter()
        .map(|text| measure_text(ui, text))
        .fold(0.0, f32::max)
}

fn left_column_width(ui: &egui::Ui) -> f32 {
    let t = crate::i18n::tr();
    let hint_labels = [
        t.hint_col_format,
        t.hint_ts,
        t.hint_filetime,
        t.hint_ipv4,
        t.hint_mac,
        t.hint_ipv6,
        t.hint_magic,
    ];
    let display_labels = [
        t.endian,
        t.cell_max_label,
        t.page_size_label,
        t.thousands_label,
        t.hex_compact_label,
        t.text_line_numbers_label,
        t.hex_line_numbers_label,
    ];
    let hint_width =
        max_text_width(ui, &hint_labels) + ui.spacing().interact_size.x * 2.0 + 16.0 * 2.0 + 16.0;
    let display_control_width =
        measure_text(ui, "00D2FF01") + ui.spacing().interact_size.x + ui.spacing().item_spacing.x;
    let display_width =
        max_text_width(ui, &display_labels) + 16.0 + display_control_width.max(72.0 + 24.0) + 16.0;
    let log_width =
        measure_text(ui, t.log_level) + 16.0 + 72.0 + ui.spacing().interact_size.x + 16.0;
    hint_width.max(display_width).max(log_width)
}

fn shortcuts_width(ui: &egui::Ui) -> f32 {
    let t = crate::i18n::tr();
    let keys = [
        "Ctrl+O",
        "Ctrl+F",
        "F3 / Shift+F3",
        "Esc",
        "PgDn / PgUp",
        "↑ / ↓",
        "Ctrl+C",
        "Enter",
    ];
    let descriptions = [
        t.sc_open,
        t.sc_search,
        t.sc_f3,
        t.sc_clear,
        t.sc_page,
        t.sc_rownav,
        t.sc_copy_row,
        t.sc_enter,
        t.sc_hex_clear,
    ];
    max_text_width(ui, &keys) + max_text_width(ui, &descriptions) + 24.0 + 16.0
}

/// 节 1：单元格附注开关表（格式 | Key | Value 勾选）。
fn hint_section(ui: &mut egui::Ui, app: &mut MdbxerApp, width: f32) {
    let t = crate::i18n::tr();

    ui.group(|ui| {
        ui.set_min_width(width);
        ui.label(egui::RichText::new(t.hint_section).strong())
            .on_hover_text(t.hint_section_tip);
        ui.add_space(4.0);

        // 「格式 | Key | Value」三列勾选表
        egui::Grid::new("hint_flags_grid")
            .striped(true)
            .num_columns(3)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new(t.hint_col_format).strong());
                ui.label(egui::RichText::new("Key").strong());
                ui.label(egui::RichText::new("Value").strong());
                ui.end_row();

                hint_row(ui, app, t.hint_ts, |f| &mut f.unix_ts);
                hint_row(ui, app, t.hint_filetime, |f| &mut f.filetime);
                hint_row(ui, app, t.hint_ipv4, |f| &mut f.ipv4);
                hint_row(ui, app, t.hint_mac, |f| &mut f.mac);
                hint_row(ui, app, t.hint_ipv6, |f| &mut f.ipv6);
                hint_row(ui, app, t.hint_magic, |f| &mut f.magic);
            });
    });
}

/// 一行附注开关：行名 + Key/Value 两个勾选框（改动即保存）。
fn hint_row(
    ui: &mut egui::Ui,
    app: &mut MdbxerApp,
    name: &str,
    sel: impl Fn(&mut crate::fmt::HintFlags) -> &mut bool,
) {
    ui.label(name);

    let mut k = *sel(&mut app.hint_key);
    if ui.checkbox(&mut k, "").changed() {
        *sel(&mut app.hint_key) = k;
        app.save_ui_prefs();
    }

    let mut v = *sel(&mut app.hint_value);
    if ui.checkbox(&mut v, "").changed() {
        *sel(&mut app.hint_value) = v;
        app.save_ui_prefs();
    }

    ui.end_row();
}

/// 节 2：显示参数（从顶栏移入：字节序/单元格长度/每页条数/千位分隔）。
fn display_section(ui: &mut egui::Ui, app: &mut MdbxerApp, width: f32) {
    let t = crate::i18n::tr();

    ui.group(|ui| {
        ui.set_min_width(width);
        ui.label(egui::RichText::new(t.disp_section).strong());
        ui.add_space(4.0);

        egui::Grid::new("display_params_grid")
            .num_columns(2)
            .spacing([16.0, 6.0])
            .show(ui, |ui| {
                // 字节序（影响整数/float/double 与自动猜测）
                ui.label(t.endian);
                let mut en = app.endian;
                let ir = egui::ComboBox::from_id_salt("set_endian")
                    .width(72.0)
                    .selected_text(en.suffix())
                    .show_ui(ui, |ui| {
                        for e in crate::fmt::Endian::ALL {
                            ui.selectable_value(&mut en, e, e.label());
                        }
                    });
                super::wheel_cycle(ui.ctx(), &ir.response, &crate::fmt::Endian::ALL, &mut en);
                if app.endian != en {
                    app.endian = en;
                    app.save_ui_prefs();
                }
                ui.end_row();

                // 单元格最多显示字符数
                ui.label(t.cell_max_label);
                let mut cm = app.cell_max;
                let ir = egui::ComboBox::from_id_salt("set_cell_max")
                    .width(72.0)
                    .selected_text(format!("{cm}"))
                    .show_ui(ui, |ui| {
                        for v in CELL_MAXES {
                            ui.selectable_value(&mut cm, *v, v.to_string());
                        }
                    });
                let resp = ir.response.on_hover_text(t.cell_max_tip);
                super::wheel_cycle(ui.ctx(), &resp, CELL_MAXES, &mut cm);
                if app.cell_max != cm {
                    app.cell_max = cm;
                    app.save_ui_prefs();
                }
                ui.end_row();

                // 每页显示条数
                ui.label(t.page_size_label);
                let mut ps = app.page_size;
                let ir = egui::ComboBox::from_id_salt("set_page_size")
                    .width(72.0)
                    .selected_text(format!("{ps}"))
                    .show_ui(ui, |ui| {
                        for v in super::PAGE_SIZES {
                            ui.selectable_value(&mut ps, v, v.to_string());
                        }
                    });
                let resp = ir.response.on_hover_text(t.page_size_tip);
                super::wheel_cycle(ui.ctx(), &resp, &super::PAGE_SIZES, &mut ps);
                if ps != app.page_size {
                    app.page_size = ps;
                    app.save_ui_prefs();
                    app.load_first_page();
                }
                ui.end_row();

                // 整数千位分隔开关（影响所有 decode/guess 输出）
                ui.label(t.thousands_label);
                let mut ts = crate::fmt::thousands_sep();
                if ui
                    .checkbox(&mut ts, "1,234")
                    .on_hover_text(t.thousands_tip)
                    .changed()
                {
                    crate::fmt::set_thousands_sep(ts);
                    app.save_ui_prefs();
                }
                ui.end_row();

                // HEX 视图紧凑格式开关（HEX 列字节间不加空格）
                ui.label(t.hex_compact_label);
                let mut hc = app.detail.hex_compact;
                if ui
                    .checkbox(&mut hc, "00D2FF01")
                    .on_hover_text(t.hex_compact_tip)
                    .changed()
                {
                    app.detail.hex_compact = hc;
                    app.save_ui_prefs();
                }
                ui.end_row();

                // 文本行号：真实文本换行；HEX 格式按显示行
                ui.label(t.text_line_numbers_label);
                if ui
                    .checkbox(&mut app.detail.show_text_line_numbers, "")
                    .on_hover_text(t.text_line_numbers_tip)
                    .changed()
                {
                    app.save_ui_prefs();
                }
                ui.end_row();

                // 独立 HexView 的显示行号
                ui.label(t.hex_line_numbers_label);
                if ui
                    .checkbox(&mut app.detail.show_hex_line_numbers, "")
                    .on_hover_text(t.hex_line_numbers_tip)
                    .changed()
                {
                    app.save_ui_prefs();
                }
                ui.end_row();
            });
    });
}

/// 单元格显示长度档位（与原顶栏一致）。
const CELL_MAXES: &[usize] = &[64, 128, 256, 512, 1024, 4096];

/// 节 3：日志（等级切换 + 打开日志目录）。
fn log_section(ui: &mut egui::Ui, width: f32) {
    let t = crate::i18n::tr();

    ui.group(|ui| {
        ui.set_min_width(width);
        ui.label(egui::RichText::new(t.log_section).strong())
            .on_hover_text(t.log_section_tip);
        ui.add_space(4.0);

        egui::Grid::new("log_grid")
            .num_columns(2)
            .spacing([16.0, 6.0])
            .show(ui, |ui| {
                ui.label(t.log_level);
                ui.horizontal(|ui| {
                    // 日志等级：切换即时生效并持久化
                    let cur = crate::logging::level();
                    let mut lv = cur;
                    let ir = egui::ComboBox::from_id_salt("set_log_level")
                        .width(72.0)
                        .selected_text(level_label(lv))
                        .show_ui(ui, |ui| {
                            for l in crate::logging::Level::ALL {
                                ui.selectable_value(&mut lv, l, level_label(l));
                            }
                        });
                    super::wheel_cycle(
                        ui.ctx(),
                        &ir.response,
                        &crate::logging::Level::ALL,
                        &mut lv,
                    );
                    if lv != cur {
                        crate::logging::set_level(lv);
                        crate::config::save_log_level(lv);
                        crate::logging::info(&format!("日志等级切换为 {}", lv.as_str()));
                    }

                    // 打开日志目录（尚未写过日志时目录可能不存在，先确保建好）
                    if let Some(dir) = crate::logging::log_dir() {
                        if ui.button("📂").on_hover_text(t.log_open_dir_tip).clicked() {
                            let _ = std::fs::create_dir_all(&dir);
                            super::topbar::reveal_in_folder(&dir.display().to_string(), false);
                        }
                    }
                });
                ui.end_row();
            });
    });
}

/// 日志等级的界面标签（随界面语言）。
fn level_label(l: crate::logging::Level) -> &'static str {
    let t = crate::i18n::tr();
    match l {
        crate::logging::Level::Error => t.log_lv_error,
        crate::logging::Level::Warn => t.log_lv_warn,
        crate::logging::Level::Info => t.log_lv_info,
        crate::logging::Level::Debug => t.log_lv_debug,
    }
}

/// 节 4：快捷键清单（只读，仅供查阅）。
fn shortcuts_section(ui: &mut egui::Ui, width: f32) {
    let t = crate::i18n::tr();

    ui.group(|ui| {
        ui.set_min_width(width);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(t.shortcut_section).strong())
                .on_hover_text(t.shortcut_section_tip);
            ui.add_space(4.0);

            let rows: [(&str, &str); 9] = [
                ("Ctrl+O", t.sc_open),
                ("Ctrl+F", t.sc_search),
                ("F3 / Shift+F3", t.sc_f3),
                ("Esc", t.sc_clear),
                ("PgDn / PgUp", t.sc_page),
                ("↑ / ↓", t.sc_rownav),
                ("Ctrl+C", t.sc_copy_row),
                ("Enter", t.sc_enter),
                ("Esc", t.sc_hex_clear),
            ];
            egui::Grid::new("shortcuts_grid")
                .num_columns(2)
                .spacing([24.0, 4.0])
                .show(ui, |ui| {
                    for (key, desc) in rows {
                        ui.label(egui::RichText::new(key).monospace().strong());
                        ui.label(desc);
                        ui.end_row();
                    }
                });
        });
    });
}
