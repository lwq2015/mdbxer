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
        // 分组边框的左右内边距：让同列下组的宽度与上组实测外宽一致
        let group_margin = egui::Frame::group(ui.style()).inner_margin.sum().x;
        // 顶部两栏的实际右边缘：底部全宽分组据此对齐
        let mut top_width = ui.available_width();
        if ui.available_width() >= left_width + column_gap + shortcuts_width + group_margin {
            let row = ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    // 实测上组外宽，下组按同宽排布，两组右边缘对齐
                    let hint_w = hint_section(ui, app, left_width);
                    ui.add_space(10.0);
                    display_section(ui, app, (hint_w - group_margin).max(left_width));
                });
                ui.add_space(column_gap);
                ui.vertical(|ui| {
                    let sc_w = shortcuts_section(ui, shortcuts_width);
                    ui.add_space(10.0);
                    log_section(ui, (sc_w - group_margin).max(shortcuts_width));
                });
            });
            top_width = row.response.rect.width();
        } else {
            hint_section(ui, app, left_width);
            ui.add_space(10.0);
            display_section(ui, app, left_width);
            ui.add_space(10.0);
            shortcuts_section(ui, shortcuts_width);
            ui.add_space(10.0);
            log_section(ui, shortcuts_width);
        }
        ui.add_space(10.0);
        value_labels_section(ui, app, (top_width - group_margin).max(left_width));
    });
    // 规则编辑子窗口（每帧独立渲染，避免被滚动区域裁剪）
    show_edit_window(ui.ctx(), app);
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
fn hint_section(ui: &mut egui::Ui, app: &mut MdbxerApp, width: f32) -> f32 {
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
    })
    .response
    .rect
    .width()
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
                    // 表统计/环境信息的值在取数时就已按分隔符格式化，缓存必须失效
                    // 才能让开关立即生效（否则要手动点"刷新"才更新）。
                    app.stat_cache = None;
                    app.env_cache = None;
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

                // 详情卡片底部校验和：单选（不选则不显示该行）
                ui.label(t.hash_label).on_hover_text(t.hash_tip);
                let mut alg = app.hash_alg;
                egui::ComboBox::from_id_salt("hash_alg")
                    .width(90.0)
                    .selected_text(alg.map(|a| a.name()).unwrap_or(t.hash_none))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut alg, None, t.hash_none);
                        for a in crate::hash::HashAlg::ALL {
                            ui.selectable_value(&mut alg, Some(a), a.name());
                        }
                    });
                if alg != app.hash_alg {
                    app.hash_alg = alg;
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
fn shortcuts_section(ui: &mut egui::Ui, width: f32) -> f32 {
    let t = crate::i18n::tr();

    ui.group(|ui| {
        ui.set_min_width(width);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(t.shortcut_section).strong())
                .on_hover_text(t.shortcut_section_tip);
            ui.add_space(4.0);

            let rows: [(&str, &str); 10] = [
                ("Ctrl+O", t.sc_open),
                ("Ctrl+F", t.sc_search),
                ("F3 / Shift+F3", t.sc_f3),
                ("F5", t.sc_refresh),
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
    })
    .response
    .rect
    .width()
}

/// 关联范围下拉的显示文本：库默认 / 主表 / 具体表名。
fn scope_label(scope: &(bool, Option<String>)) -> String {
    let t = crate::i18n::tr();
    match scope {
        (false, _) => t.vlabel_scope_db.to_string(),
        (true, None) => t.main_table.to_string(),
        (true, Some(name)) => name.clone(),
    }
}

/// 标注规则编辑子窗口状态：draft 在点「确定」时才写回列表。
pub(crate) struct VLabelEdit {
    /// 被编辑规则的下标；None = 新增
    pub index: Option<usize>,
    pub draft: crate::config::ValueLabel,
}

/// 列表预览文本：超长截断（悬停可看全文）。
fn clip_text(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// 关联候选：库默认 + 主表 + 各表（主表 name = None）。
fn scope_choices(app: &MdbxerApp) -> Vec<(bool, Option<String>)> {
    let mut scopes: Vec<(bool, Option<String>)> = vec![(false, None)];
    if let Some(h) = &app.db {
        for tb in &h.tables {
            let s = (true, tb.name.clone());
            if !scopes.contains(&s) {
                scopes.push(s);
            }
        }
    }
    if !scopes.iter().any(|s| s.0) {
        scopes.push((true, None));
    }
    scopes
}

/// 定义文本的悬停预览：每条定义一行，最多 64 个位标识也不会挤成一行。
fn defs_tip(defs: &str) -> String {
    defs.split([',', ';', '\n', '\r'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 节 5：Value 整数标注规则列表（可添加、删除、修改、禁用）。
fn value_labels_section(ui: &mut egui::Ui, app: &mut MdbxerApp, width: f32) {
    let t = crate::i18n::tr();

    ui.group(|ui| {
        ui.set_min_width(width);
        ui.label(egui::RichText::new(t.vlabel_section).strong())
            .on_hover_text(t.vlabel_section_tip);
        ui.add_space(4.0);

        if app.opened_path.is_none() {
            ui.label(t.vlabel_no_db);
            return;
        }

        let scopes = scope_choices(app);

        // 取出规则列表以便逐行编辑（避免与 app 其他字段的借用冲突）
        let mut labels = std::mem::take(&mut app.value_labels);
        let mut changed = false;
        let mut to_delete = None;
        let mut to_edit: Option<usize> = None;

        egui::Grid::new("value_labels_grid")
            .striped(true)
            .num_columns(5)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new(t.vlabel_scope).strong());
                ui.label(egui::RichText::new(t.vlabel_key).strong());
                ui.label(egui::RichText::new(t.vlabel_kind).strong());
                ui.label(egui::RichText::new(t.vlabel_defs).strong());
                ui.label("");
                ui.end_row();

                for (i, l) in labels.iter_mut().enumerate() {
                    // 关联范围
                    let mut scope = (l.per_table, l.table.clone());
                    egui::ComboBox::from_id_salt(("vlabel_scope", i))
                        .width(140.0)
                        .selected_text(scope_label(&scope))
                        .show_ui(ui, |ui| {
                            for s in &scopes {
                                ui.selectable_value(&mut scope, s.clone(), scope_label(s));
                            }
                        });
                    if scope != (l.per_table, l.table.clone()) {
                        l.per_table = scope.0;
                        l.table = scope.1;
                        changed = true;
                    }

                    // 目标 Key：截断预览 + 悬停全文（完整编辑在子窗口）
                    if l.key.is_empty() {
                        ui.label(egui::RichText::new("—").weak());
                    } else {
                        ui.label(clip_text(&l.key, 28)).on_hover_text(&l.key);
                    }

                    // 解释方式：位标识 / 枚举
                    let mut is_enum = l.kind == "enum";
                    egui::ComboBox::from_id_salt(("vlabel_kind", i))
                        .width(84.0)
                        .selected_text(if is_enum {
                            t.vlabel_kind_enum
                        } else {
                            t.vlabel_kind_bits
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut is_enum, false, t.vlabel_kind_bits);
                            ui.selectable_value(&mut is_enum, true, t.vlabel_kind_enum);
                        });
                    if is_enum != (l.kind == "enum") {
                        l.kind = if is_enum { "enum" } else { "bits" }.to_string();
                        changed = true;
                    }

                    // 定义文本：截断预览 + 悬停全文（每条一行）
                    if l.defs.is_empty() {
                        ui.label(egui::RichText::new("—").weak());
                    } else {
                        ui.label(clip_text(&l.defs, 60))
                            .on_hover_text(defs_tip(&l.defs));
                    }

                    // 启用开关（悬浮提示说明当前状态，省去列标题）+ 编辑 / 删除
                    ui.horizontal(|ui| {
                        let tip = if l.enabled {
                            t.vlabel_enabled
                        } else {
                            t.vlabel_disabled
                        };
                        if ui.checkbox(&mut l.enabled, "").on_hover_text(tip).changed() {
                            changed = true;
                        }
                        if ui.small_button("✏").on_hover_text(t.vlabel_edit).clicked() {
                            to_edit = Some(i);
                        }
                        if ui.small_button("🗑").on_hover_text(t.vlabel_del).clicked() {
                            to_delete = Some(i);
                        }
                    });
                    ui.end_row();
                }
            });

        app.value_labels = labels;

        if let Some(i) = to_edit {
            if let Some(l) = app.value_labels.get(i) {
                app.vlabel_edit = Some(VLabelEdit {
                    index: Some(i),
                    draft: l.clone(),
                });
            }
        }
        if let Some(i) = to_delete {
            if i < app.value_labels.len() {
                app.value_labels.remove(i);
                changed = true;
            }
        }

        ui.horizontal(|ui| {
            if ui.button(t.vlabel_add).clicked() {
                app.vlabel_edit = Some(VLabelEdit {
                    index: None,
                    draft: crate::config::ValueLabel::default(),
                });
            }
        });
        ui.label(egui::RichText::new(t.vlabel_format_tip).weak());

        if changed {
            app.views_key = None;
            app.save_per_db();
        }
    });
}

/// 规则编辑子窗口：多行输入，确定后写回并立即刷新表格显示。
fn show_edit_window(ctx: &egui::Context, app: &mut MdbxerApp) {
    let Some(mut edit) = app.vlabel_edit.take() else {
        return;
    };
    let t = crate::i18n::tr();
    let scopes = scope_choices(app);

    let mut open = true;
    let mut apply = false;
    let mut cancel = false;

    egui::Window::new(t.vlabel_edit_title)
        .id(egui::Id::new("vlabel_edit_window"))
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .default_height(440.0)
        .min_width(420.0)
        .min_height(280.0)
        .open(&mut open)
        .show(ctx, |ui| {
            // 底部固定区（格式说明 + 按钮）：窗口缩放时始终贴底
            egui::Panel::bottom("vlabel_edit_bottom")
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(t.vlabel_format_tip).weak());
                    ui.add_space(6.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(t.cancel).clicked() {
                            cancel = true;
                        }
                        if ui.button(t.vlabel_ok).clicked() {
                            apply = true;
                        }
                    });
                });

            // 上方编辑区：Key/定义宽度跟随窗口，定义高度填满剩余空间
            egui::CentralPanel::no_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(t.vlabel_scope);
                    let mut scope = (edit.draft.per_table, edit.draft.table.clone());
                    egui::ComboBox::from_id_salt("vlabel_edit_scope")
                        .width(220.0)
                        .selected_text(scope_label(&scope))
                        .show_ui(ui, |ui| {
                            for s in &scopes {
                                ui.selectable_value(&mut scope, s.clone(), scope_label(s));
                            }
                        });
                    edit.draft.per_table = scope.0;
                    edit.draft.table = scope.1;

                    ui.add_space(12.0);
                    ui.checkbox(&mut edit.draft.enabled, t.vlabel_enabled);
                });

                ui.add_space(6.0);
                ui.label(t.vlabel_key);
                ui.add(
                    egui::TextEdit::multiline(&mut edit.draft.key)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .hint_text("SortNo"),
                );

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(t.vlabel_kind);
                    let mut is_enum = edit.draft.kind == "enum";
                    egui::ComboBox::from_id_salt("vlabel_edit_kind")
                        .width(160.0)
                        .selected_text(if is_enum {
                            t.vlabel_kind_enum
                        } else {
                            t.vlabel_kind_bits
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut is_enum, false, t.vlabel_kind_bits);
                            ui.selectable_value(&mut is_enum, true, t.vlabel_kind_enum);
                        });
                    edit.draft.kind = if is_enum { "enum" } else { "bits" }.to_string();
                });

                ui.add_space(6.0);
                ui.label(t.vlabel_defs);
                let h = ui.available_height().max(60.0);
                ui.add_sized(
                    [ui.available_width(), h],
                    egui::TextEdit::multiline(&mut edit.draft.defs)
                        .desired_rows(1)
                        .hint_text("0=Read,1=Write"),
                );
            });
        });

    if apply {
        match edit.index {
            Some(i) if i < app.value_labels.len() => app.value_labels[i] = edit.draft,
            _ => app.value_labels.push(edit.draft),
        }
        app.views_key = None;
        app.save_per_db();
    } else if open && !cancel {
        // 窗口仍开着（例如点了行内其他控件）：保留草稿
        app.vlabel_edit = Some(edit);
    }
}
