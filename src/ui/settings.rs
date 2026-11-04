// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 高级配置页（中央页签）：拆成「显示参数」「标识」两个子页。
//! 各开关改动即时生效并持久化（save_ui_prefs）；附注开关变化会经 views_key
//! 失效自动重建当前页显示缓存。

use super::MdbxerApp;

/// 子页签 id（保存在 egui 内存，切页/切语言不丢当前子页）
const ADV_TAB_ID: &str = "mdbxer_adv_tab";

/// 高级配置页主体。
pub(crate) fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let t = crate::i18n::tr();
    let mut tab = ui
        .ctx()
        .data_mut(|d| d.get_temp::<usize>(egui::Id::new(ADV_TAB_ID)).unwrap_or(0));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut tab, 0, t.disp_section);
        ui.selectable_value(&mut tab, 1, t.vlabel_tab);
    });
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new(ADV_TAB_ID), tab));
    ui.separator();

    // 标注子页的行数多、列数多（颜色规则需足够宽度），窗口偏窄时允许横向滚动，
    // 避免右侧输入框被裁切
    egui::ScrollArea::both().show(ui, |ui| {
        ui.add_space(8.0);
        if tab == 1 {
            labels_page(ui, app);
        } else {
            display_page(ui, app);
        }
    });
    // 规则编辑子窗口（每帧独立渲染，避免被滚动区域裁剪）
    show_edit_window(ui.ctx(), app);
    show_type_edit_window(ui.ctx(), app);
}

/// 子页「显示参数」：附注开关 + 显示参数 + 日志（右栏快捷键）。
fn display_page(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let left_width = left_column_width(ui);
    let shortcuts_width = shortcuts_width(ui);
    let column_gap = 8.0;
    // 分组边框的左右内边距：让同列下组的宽度与上组实测外宽一致
    let group_margin = egui::Frame::group(ui.style()).inner_margin.sum().x;
    if ui.available_width() >= left_width + column_gap + shortcuts_width + group_margin {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                // 实测上组外宽，下组按同宽排布，两组右边缘对齐
                let hint_w = hint_section(ui, app, left_width);
                ui.add_space(10.0);
                display_section(ui, app, (hint_w - group_margin).max(left_width));
                ui.add_space(10.0);
                log_section(ui, (hint_w - group_margin).max(left_width));
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
}

/// 「标注」子页三组共用的最小宽度：颜色规则列最多，按内容给足，三组同宽对齐
const LABELS_GROUP_WIDTH: f32 = 660.0;

/// 子页「标注」：通用类型 + Value 整数标注 + 颜色规则。
fn labels_page(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let left_width = left_column_width(ui);
    let shortcuts_width = shortcuts_width(ui);
    let column_gap = 8.0;
    let group_margin = egui::Frame::group(ui.style()).inner_margin.sum().x;
    let total_width = (left_width + column_gap + shortcuts_width).min(ui.available_width());
    let group_width = (total_width - group_margin)
        .max(left_width)
        .max(LABELS_GROUP_WIDTH);

    label_types_section(ui, app, group_width);
    ui.add_space(10.0);
    value_labels_section(ui, app, group_width);
    ui.add_space(10.0);
    color_rules_section(ui, app, group_width);
}

/// 「标注」子页各分组共用的可折叠标题：默认展开，展开状态存 egui 内存
/// （切页/切语言不丢）。返回当前是否展开，折叠时调用方跳过正文，
/// 规则较多时可逐个收起。
fn collapsible_section_header(ui: &mut egui::Ui, id_salt: &str, title: &str, tip: &str) -> bool {
    let id = egui::Id::new(("adv_section_open", id_salt));
    let mut open = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(true);
    ui.horizontal(|ui| {
        // 三角用 egui 自带的矢量图标绘制，避免 ▾/▸ 字形在 CJK 字体里缺失显示成方框；
        // 图标按 rect 的 0.75 绘制，分配 11 点即可得到与正文协调的小三角
        let size = egui::vec2(11.0, 11.0);
        let (_, arrow_resp) = ui.allocate_exact_size(size, egui::Sense::click());
        let arrow_resp = arrow_resp.on_hover_text(tip);
        egui::collapsing_header::paint_default_icon(ui, if open { 1.0 } else { 0.0 }, &arrow_resp);
        if arrow_resp.clicked() {
            open = !open;
        }
        if ui
            .add(egui::Label::new(egui::RichText::new(title).strong()).sense(egui::Sense::click()))
            .on_hover_text(tip)
            .clicked()
        {
            open = !open;
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, open));
    open
}

/// 定宽网格单元格：Grid 里控件的可用宽度取自上一帧列宽，TextEdit 的
/// desired_width 会被压回该列宽，列宽于是永远撑不开（表现为输入框被夹成一条缝）。
/// 先按固定尺寸分配子 ui 再放控件，列宽才能按期望值稳定撑开。
fn fixed_cell<R>(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(width);
            add(ui)
        },
    )
    .inner
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

/// 通用类型编辑子窗口状态：draft 在点「确定」时才写回全局列表。
pub(crate) struct VLabelTypeEdit {
    /// 被编辑类型的下标；None = 新增
    pub index: Option<usize>,
    pub draft: crate::config::ValueLabelType,
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
        if !collapsible_section_header(ui, "value_labels", t.vlabel_section, t.vlabel_section_tip) {
            return;
        }
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

                    // 定义文本：截断预览 + 悬停全文（每组一行）
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
                        .hint_text("3..=4=[0:无, 1:对, 2:不对]\n0=Read,1=Write"),
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

/// 节 6：全局通用类型（命名位标识/枚举定义，规则用 @名称 引用）。
/// 全局共享，跨库生效；改动即保存并触发表格显示缓存重建。
fn label_types_section(ui: &mut egui::Ui, app: &mut MdbxerApp, width: f32) {
    let t = crate::i18n::tr();

    ui.group(|ui| {
        ui.set_min_width(width);
        if !collapsible_section_header(ui, "label_types", t.vtype_section, t.vtype_section_tip) {
            return;
        }
        ui.add_space(4.0);

        let mut types = std::mem::take(&mut app.label_types);
        let mut changed = false;
        let mut to_delete = None;
        let mut to_edit: Option<usize> = None;

        egui::Grid::new("label_types_grid")
            .striped(true)
            .num_columns(3)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new(t.vtype_name).strong());
                ui.label(egui::RichText::new(t.vlabel_defs).strong());
                ui.label("");
                ui.end_row();

                for (i, g) in types.iter_mut().enumerate() {
                    // 名称
                    if g.name.is_empty() {
                        ui.label(egui::RichText::new("—").weak());
                    } else {
                        ui.label(clip_text(&g.name, 20)).on_hover_text(&g.name);
                    }

                    // 定义：截断预览 + 悬停全文
                    if g.defs.is_empty() {
                        ui.label(egui::RichText::new("—").weak());
                    } else {
                        ui.label(clip_text(&g.defs, 60))
                            .on_hover_text(defs_tip(&g.defs));
                    }

                    // 启用 + 编辑 / 删除
                    ui.horizontal(|ui| {
                        let tip = if g.enabled {
                            t.vlabel_enabled
                        } else {
                            t.vlabel_disabled
                        };
                        if ui.checkbox(&mut g.enabled, "").on_hover_text(tip).changed() {
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

        app.label_types = types;

        if let Some(i) = to_edit {
            if let Some(g) = app.label_types.get(i) {
                app.vlabel_type_edit = Some(VLabelTypeEdit {
                    index: Some(i),
                    draft: g.clone(),
                });
            }
        }
        if let Some(i) = to_delete {
            if i < app.label_types.len() {
                app.label_types.remove(i);
                changed = true;
            }
        }

        ui.horizontal(|ui| {
            if ui.button(t.vtype_add).clicked() {
                app.vlabel_type_edit = Some(VLabelTypeEdit {
                    index: None,
                    draft: crate::config::ValueLabelType::default(),
                });
            }
        });
        ui.label(egui::RichText::new(t.vtype_defs_tip).weak());

        if changed {
            app.views_key = None;
            crate::config::save_label_types(&app.label_types);
        }
    });
}

/// 节 7：Value 颜色规则（Key 通配符 + 数值范围 → 颜色），行内直接编辑。
fn color_rules_section(ui: &mut egui::Ui, app: &mut MdbxerApp, width: f32) {
    let t = crate::i18n::tr();

    ui.group(|ui| {
        ui.set_min_width(width);
        if !collapsible_section_header(ui, "color_rules", t.color_section, t.color_section_tip) {
            return;
        }
        ui.add_space(4.0);

        if app.opened_path.is_none() {
            ui.label(t.vlabel_no_db);
            return;
        }

        let scopes = scope_choices(app);
        let cur_table = app.cur_table().and_then(|tb| tb.name.clone());

        let mut rules = std::mem::take(&mut app.color_rules);
        let mut changed = false;
        let mut to_delete = None;

        // 列宽按组宽分配：关联/操作列为固定窄列，Key 模式、范围、颜色按比例分剩余空间，
        // 避免可编辑列被挤成一条缝、内容看不全
        let scope_w = 120.0;
        let ops_w = 52.0;
        let rest = (width - scope_w - ops_w - 40.0).max(320.0);
        let key_w = (rest * 0.32).max(110.0);
        let range_w = (rest * 0.32).max(110.0);
        let color_w = (rest - key_w - range_w).max(150.0);
        let hex_w = (color_w - 30.0).max(96.0);

        egui::Grid::new("color_rules_grid")
            .striped(true)
            .num_columns(5)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                fixed_cell(ui, scope_w, |ui| {
                    ui.label(egui::RichText::new(t.vlabel_scope).strong());
                });
                fixed_cell(ui, key_w, |ui| {
                    ui.label(egui::RichText::new(t.color_key).strong())
                        .on_hover_text(t.color_section_tip);
                });
                fixed_cell(ui, range_w, |ui| {
                    ui.label(egui::RichText::new(t.color_range).strong())
                        .on_hover_text(t.color_tip);
                });
                fixed_cell(ui, color_w, |ui| {
                    ui.label(egui::RichText::new(t.color_value).strong())
                        .on_hover_text(t.color_tip);
                });
                fixed_cell(ui, ops_w, |_ui| {});
                ui.end_row();

                for (i, r) in rules.iter_mut().enumerate() {
                    // 关联范围
                    let mut scope = (r.per_table, r.table.clone());
                    fixed_cell(ui, scope_w, |ui| {
                        egui::ComboBox::from_id_salt(("color_scope", i))
                            .width(scope_w - 24.0)
                            .selected_text(scope_label(&scope))
                            .show_ui(ui, |ui| {
                                for s in &scopes {
                                    ui.selectable_value(&mut scope, s.clone(), scope_label(s));
                                }
                            });
                    });
                    if scope != (r.per_table, r.table.clone()) {
                        r.per_table = scope.0;
                        r.table = scope.1;
                        changed = true;
                    }

                    // Key 模式：glob 通配符
                    let key_resp = fixed_cell(ui, key_w, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut r.key)
                                .desired_width(key_w)
                                .hint_text("key_*"),
                        )
                        .on_hover_text(t.color_section_tip)
                    });
                    if key_resp.changed() {
                        changed = true;
                    }

                    // 数值范围
                    let range_resp = fixed_cell(ui, range_w, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut r.range)
                                .desired_width(range_w)
                                .hint_text("<10  [10,20)"),
                        )
                        .on_hover_text(t.color_tip)
                    });
                    if range_resp.changed() {
                        changed = true;
                    }

                    // 颜色：色块取色 + hex 文本
                    fixed_cell(ui, color_w, |ui| {
                        let mut col =
                            super::parse_hex_color(&r.color).unwrap_or(egui::Color32::GRAY);
                        if ui
                            .color_edit_button_srgba(&mut col)
                            .on_hover_text(t.color_tip)
                            .changed()
                        {
                            r.color = format!("#{:02X}{:02X}{:02X}", col.r(), col.g(), col.b());
                            changed = true;
                        }
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut r.color)
                                    .desired_width(hex_w)
                                    .hint_text("#RRGGBB"),
                            )
                            .on_hover_text(t.color_tip)
                            .changed()
                        {
                            changed = true;
                        }
                    });

                    // 启用开关（悬浮提示当前状态）+ 删除
                    fixed_cell(ui, ops_w, |ui| {
                        let tip = if r.enabled {
                            t.vlabel_enabled
                        } else {
                            t.vlabel_disabled
                        };
                        if ui.checkbox(&mut r.enabled, "").on_hover_text(tip).changed() {
                            changed = true;
                        }
                        if ui.small_button("🗑").on_hover_text(t.vlabel_del).clicked() {
                            to_delete = Some(i);
                        }
                    });
                    ui.end_row();
                }
            });

        app.color_rules = rules;

        if let Some(i) = to_delete {
            if i < app.color_rules.len() {
                app.color_rules.remove(i);
                changed = true;
            }
        }

        ui.horizontal(|ui| {
            if ui.button(t.color_add).clicked() {
                app.color_rules.push(crate::config::ColorRule {
                    per_table: true,
                    table: cur_table.clone(),
                    key: String::new(),
                    range: String::new(),
                    color: "#FFD24A".to_string(),
                    enabled: true,
                });
                changed = true;
            }
        });
        ui.label(egui::RichText::new(t.color_tip).weak());

        if changed {
            app.views_key = None;
            app.save_per_db();
        }
    });
}

/// 通用类型编辑子窗口：名称 + 多行定义，确定后写回全局列表。
fn show_type_edit_window(ctx: &egui::Context, app: &mut MdbxerApp) {
    let Some(mut edit) = app.vlabel_type_edit.take() else {
        return;
    };
    let t = crate::i18n::tr();

    let mut open = true;
    let mut apply = false;
    let mut cancel = false;

    egui::Window::new(t.vtype_edit_title)
        .id(egui::Id::new("vlabel_type_edit_window"))
        .collapsible(false)
        .resizable(true)
        .default_width(480.0)
        .default_height(320.0)
        .min_width(360.0)
        .min_height(220.0)
        .open(&mut open)
        .show(ctx, |ui| {
            egui::Panel::bottom("vtype_edit_bottom")
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(t.vtype_defs_tip).weak());
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

            egui::CentralPanel::no_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(t.vtype_name);
                    ui.add(
                        egui::TextEdit::singleline(&mut edit.draft.name)
                            .desired_width(200.0)
                            .hint_text("协议"),
                    );
                    ui.add_space(12.0);
                    ui.checkbox(&mut edit.draft.enabled, t.vlabel_enabled);
                });

                ui.add_space(6.0);
                ui.label(t.vlabel_defs);
                let h = ui.available_height().max(60.0);
                ui.add_sized(
                    [ui.available_width(), h],
                    egui::TextEdit::multiline(&mut edit.draft.defs)
                        .desired_rows(1)
                        .hint_text("0=TCP,1=UDP,2=ICMP"),
                );
            });
        });

    if apply {
        match edit.index {
            Some(i) if i < app.label_types.len() => app.label_types[i] = edit.draft,
            _ => app.label_types.push(edit.draft),
        }
        app.views_key = None;
        crate::config::save_label_types(&app.label_types);
    } else if open && !cancel {
        app.vlabel_type_edit = Some(edit);
    }
}
