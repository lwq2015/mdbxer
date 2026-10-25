// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 中间 "数据" 页签：工具条 + 表格。

use super::{MdbxerApp, SortCol};
use crate::fmt::{self, DecodeMode, Endian, HintFlags};
use crate::i18n::tr;
use egui_extras::{Column, TableBuilder};

/// 列头单元格：文字与整列空白都可点击。返回是否被点击。
fn header_cell(ui: &mut egui::Ui, text: &str, tip: &str) -> bool {
    let r1 = ui
        .add(egui::Label::new(egui::RichText::new(text).strong()).sense(egui::Sense::click()))
        .on_hover_text(tip);
    // 覆盖列内剩余空白区域，使整列都可点击、可悬停看提示
    let r2 = ui
        .allocate_rect(ui.available_rect_before_wrap(), egui::Sense::click())
        .on_hover_text(tip);
    r1.clicked() || r2.clicked()
}

/// tooltip 内容至少显示的字符数（长内容单元格悬停时可见更多原文）
const TOOLTIP_CHARS: usize = 512;

/// tooltip 最大宽度：等宽字体下让 512 字尽量少折行
const TOOLTIP_MAX_WIDTH: f32 = 720.0;

/// tooltip 最大高度：同时受窗口高度约束，避免内容撑出屏幕。
const TOOLTIP_MAX_HEIGHT: f32 = 640.0;

/// 文本按等宽字体排版后是否宽于单元格可用宽度。
fn text_is_elided(ui: &egui::Ui, resp: &egui::Response, full: &str) -> bool {
    let mono = egui::FontId::monospace(ui.style().text_styles[&egui::TextStyle::Body].size);
    let width = ui.fonts_mut(|f| {
        f.layout_no_wrap(full.to_string(), mono, egui::Color32::WHITE)
            .size()
            .x
    });
    width > resp.rect.width()
}

/// 截取前 `max` 个字符（超出补省略号）。
fn preview_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect::<String>() + "…"
    }
}

/// tooltip 正文：等宽、默认色（避免选中行白字白底），限定宽高后可滚动。
fn tooltip_body(ui: &mut egui::Ui, text: &str) {
    ui.set_max_width(TOOLTIP_MAX_WIDTH);
    let max_height = (ui.ctx().viewport_rect().height() * 0.8).min(TOOLTIP_MAX_HEIGHT);
    egui::ScrollArea::vertical()
        .max_height(max_height)
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(text)
                        .monospace()
                        .color(ui.visuals().text_color()),
                )
                .selectable(false)
                .wrap(),
            );
        });
}

/// 单元格 hover 提示：文本被列宽截断时弹完整文本（最多 TOOLTIP_CHARS 字）。
fn elide_tooltip(ui: &egui::Ui, resp: egui::Response, full: &str) {
    if !text_is_elided(ui, &resp, full) {
        return;
    }
    let text = preview_chars(full, TOOLTIP_CHARS);
    resp.on_hover_ui(move |ui| tooltip_body(ui, &text));
}

/// Key/Value 单元格 hover 提示。
///
/// 两种"没显示完整"的情况都弹：
/// - 显示文本被列宽截断；
/// - 原文超过 cell_max 被截断（即使截断后的文本正好放得下）。
///
/// 内容按原始字节重新解码到 TOOLTIP_CHARS 字，比单元格里被 cell_max
/// 截断的预览更长；解码放在 tooltip 闭包里，只在悬停该单元格时才执行。
#[allow(clippy::too_many_arguments)]
fn cell_tooltip(
    ui: &egui::Ui,
    resp: egui::Response,
    row_idx: usize,
    raw: &[u8],
    visible: &str,
    clipped: bool,
    is_key: bool,
    mode: DecodeMode,
    endian: Endian,
    hints: HintFlags,
) {
    if !clipped && !text_is_elided(ui, &resp, visible) {
        return;
    }

    let visible = visible.to_string();
    resp.on_hover_ui(move |ui| {
        let text = cached_preview(
            ui.ctx(),
            row_idx,
            is_key,
            visible,
            mode,
            endian,
            hints,
            || {
                if is_key {
                    fmt::decode_key(raw, mode, endian, TOOLTIP_CHARS, &hints)
                } else {
                    let text =
                        super::detail::tooltip_value_text(raw, mode, endian, &hints, TOOLTIP_CHARS);
                    preview_chars(&text, TOOLTIP_CHARS)
                }
            },
        );
        tooltip_body(ui, &text);
    });
}

/// 悬停预览缓存键：行号 + 是否 Key + 单元格当前显示文本。
type PreviewKey = (usize, bool, String, DecodeMode, Endian, HintFlags);

/// 悬停期间复用预览文本。Auto 解码会解压压缩容器，大字段不能每帧重算；
/// 显示文本、解码格式、字节序或附注开关变化时缓存键自然失效。
fn cached_preview(
    ctx: &egui::Context,
    row_idx: usize,
    is_key: bool,
    visible: String,
    mode: DecodeMode,
    endian: Endian,
    hints: HintFlags,
    decode: impl FnOnce() -> String,
) -> String {
    let id = egui::Id::new("mdbxer_cell_tooltip");
    let key: PreviewKey = (row_idx, is_key, visible, mode, endian, hints);
    if let Some(text) = ctx.data(|d| {
        d.get_temp::<(PreviewKey, String)>(id)
            .filter(|(k, _)| *k == key)
            .map(|(_, text)| text)
    }) {
        return text;
    }

    let text = decode();
    ctx.data_mut(|d| d.insert_temp(id, (key, text.clone())));
    text
}

/// "数据"页签入口：工具条 + 行表格 + 列头排序处理。
pub fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let t = tr();
    if app.cur_table().is_none() {
        ui.label(t.select_table);
        return;
    }

    // ── 工具条（单行紧凑：符号按钮 + 悬停说明；范围信息见底部状态栏）──
    // 每页条数在顶栏；导出当前表在左栏表区域；Key 跳转/过滤统一到搜索框
    ui.horizontal(|ui| {
        // 默认顺序 = 表中读取出来的顺序（正向遍历）
        let sort_text = match app.col_sort {
            Some((col, asc)) => {
                format!("{} {}", col.label(), if asc { "⬆" } else { "⬇" })
            }
            None => {
                if app.sort_desc {
                    "Key ⬇".to_string()
                } else {
                    t.default_order.to_string()
                }
            }
        };
        ui.add(egui::Label::new(sort_text).sense(egui::Sense::hover()))
            .on_hover_text(t.sort_hint);
        let is_default = !app.sort_desc && app.col_sort.is_none();
        if ui
            .add_enabled(!is_default, egui::Button::new("↺"))
            .on_hover_text(t.reset_order)
            .clicked()
        {
            app.sort_desc = false;
            app.col_sort = None;
            app.load_first_page();
        }

        ui.separator();
        if ui.button("⏮").on_hover_text(t.tip_first).clicked() {
            app.load_first_page();
        }
        if ui
            .add_enabled(!app.at_start, egui::Button::new("◀"))
            .on_hover_text(t.tip_prev)
            .clicked()
        {
            app.load_prev_page();
        }
        if ui
            .add_enabled(!app.at_end, egui::Button::new("▶"))
            .on_hover_text(t.tip_next)
            .clicked()
        {
            app.load_next_page();
        }
        if ui.button("⏭").on_hover_text(t.tip_last).clicked() {
            app.load_last_page();
        }

        ui.separator();
        // Value 搜索模式开关：放在搜索框前（先选模式再输入），
        // 提示与主题切换一样按状态分开（提示将切换到的模式）
        if ui
            .checkbox(&mut app.value_search_full, t.full_search_toggle)
            .on_hover_text(if app.value_search_full {
                t.full_search_tip_on
            } else {
                t.full_search_tip_off
            })
            .changed()
        {
            // 切到全表模式时清除页内过滤残留，避免干扰
            if app.value_search_full {
                app.clear_value_search();
            }
        }

        // Key 搜索模式开关：➡ 跳转定位 / F 前缀过滤（F 与 K/V 按钮风格一致，
        // 避免放大镜图标被误认成"搜索"）。
        // 放在搜索框前（先选模式再输入），提示与主题切换一样按状态分开。
        if ui
            .button(if app.key_filter_mode { "F" } else { "➡" })
            .on_hover_text(if app.key_filter_mode {
                t.key_mode_tip_on
            } else {
                t.key_mode_tip_off
            })
            .clicked()
        {
            app.key_filter_mode = !app.key_filter_mode;
        }

        // 共享搜索框：Key（跳转/前缀过滤）和 Value（全表/页内文本搜索）共用
        // 符号取自 egui 内置字体 emoji-icon/NotoEmoji，跨平台不依赖系统字体
        let sresp = ui
            .add(
                egui::TextEdit::singleline(&mut app.search_input)
                    .id_salt("search_box")
                    .desired_width(160.0)
                    .hint_text(t.toolbar_search_hint),
            )
            .on_hover_text(t.search_box_tip);
        // Ctrl+F 的延迟聚焦：在实际控件上 request_focus（对猜测的 ID 直接
        // request_focus 会因 ID 不存在触发 accesskit panic）
        if app.focus_search {
            sresp.request_focus();
            app.focus_search = false;
        }
        app.search_box_id = Some(sresp.id);
        // 搜索框聚焦 = Ctrl+C 归框内文本复制，解除行复制武装
        if sresp.gained_focus() {
            app.row_copy_pending = false;
        }
        // 回车 = Key 搜索（跳转或前缀过滤，取决于模式）；
        // Ctrl+回车 = Value 搜索（行为同 V 按钮，由"全表"开关决定）。
        // 注意：egui TextEdit 只在无修饰 Enter 时 surrender focus，
        // 所以 Ctrl+Enter 不能用 lost_focus() 判断——此时焦点仍在框内（has_focus）。
        // 回车后保留焦点，连续回车不必重新点框（F3 也可全局继续）
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let ctrl = ui.input(|i| i.modifiers.ctrl);
        if enter {
            if sresp.lost_focus() {
                // 无修饰 Enter → Key 搜索
                app.apply_key_search();
                sresp.request_focus();
            } else if sresp.has_focus() && ctrl {
                // Ctrl+Enter → Value 搜索
                if app.value_search_full {
                    app.start_full_value_search();
                } else {
                    app.apply_value_search();
                }
                sresp.request_focus();
            }
        }
        // 搜索历史下拉：当前表最近的搜索词（最多 20 条），点击即填入搜索框
        if !app.search_history.is_empty() {
            let mut picked: Option<String> = None;
            ui.menu_button("🕘", |ui| {
                ui.set_min_width(160.0);
                for term in &app.search_history {
                    if ui
                        .add(
                            egui::Button::new(term)
                                .truncate()
                                .min_size(egui::vec2(140.0, 0.0)),
                        )
                        .clicked()
                    {
                        picked = Some(term.clone());
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text(t.search_history_tip);
            if let Some(term) = picked {
                app.search_input = term;
                sresp.request_focus();
            }
        }
        // 搜索 Key
        if ui.button("K").on_hover_text(t.key_search_btn_tip).clicked() {
            app.apply_key_search();
        }
        // 搜索 Value：行为由"全表"开关决定，提示按当前模式分状态
        let v_btn = ui.button("V").on_hover_text(if app.value_search_full {
            t.value_search_tip_on
        } else {
            t.value_search_tip_off
        });
        if v_btn.clicked() {
            if app.value_search_full {
                app.start_full_value_search();
            } else {
                app.apply_value_search();
            }
        }
        // 过滤状态指示 + 清除
        if app.key_filter.is_some() || app.value_filter.is_some() {
            ui.label(egui::RichText::new(t.filter_active).color(ui.visuals().warn_fg_color));
            if ui.button("×").on_hover_text(t.filter_clear_tip).clicked() {
                app.clear_key_search();
                app.clear_value_search();
            }
        }
    });

    ui.separator();

    // ── 表格 ────────────────────────────────────────────────────
    let text_height = egui::TextStyle::Body.resolve(ui.style()).size + 4.0;
    let order = app.display_order();
    let total_rows = order.len();
    let selected = app.selected_row;
    // 消费一次性滚动请求（键盘 ↑↓ / F3 命中 / 翻页回顶登记的显示行号）。
    // align=None：只在目标行不可见时滚到刚好可见，可见时不打断手动滚动。
    let scroll_to = app.pending_scroll.take().filter(|&di| di < total_rows);
    let sort_desc = app.sort_desc;
    let col_sort = app.col_sort;

    // 列头标题（带排序指示；默认读取顺序时不显示箭头）
    let key_title = if sort_desc { "Key ⬇" } else { "Key" };
    let col_title = |col: SortCol, base: &str| match col_sort {
        Some((c, true)) if c == col => format!("{base} ⬆"),
        Some((c, false)) if c == col => format!("{base} ⬇"),
        _ => base.to_string(),
    };
    let index_title = col_title(SortCol::Index, "#");
    let type_title = col_title(SortCol::Type, &SortCol::Type.label());
    let value_title = col_title(SortCol::Value, "Value");

    // 列头被点击的列：0=# 1=Key 2=类型 3=Value
    let mut col_clicked: Option<u8> = None;
    let mut clicked_row = None;

    // 选中行用浅色底 + 默认文字色（不再反白），
    // 截断 tooltip 等继承颜色的场景就不会再出现白字白底；scope 内生效不外泄
    ui.scope(|ui| {
        let bg = if ui.visuals().dark_mode {
            egui::Color32::from_rgb(0x2F, 0x4A, 0x66)
        } else {
            egui::Color32::from_rgb(0xC8, 0xDC, 0xF0)
        };
        ui.style_mut().visuals.selection.bg_fill = bg;
        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(56.0))
            .column(Column::initial(160.0).at_least(80.0).clip(true))
            // .column(Column::exact(80.0))
            .column(Column::initial(80.0).at_most(120.))
            .column(Column::remainder().clip(true))
            .min_scrolled_height(0.0);
        if let Some(di) = scroll_to {
            table = table.scroll_to_row(di, None);
        }
        table
            .header(text_height, |mut header| {
                header.col(|ui| {
                    if header_cell(ui, &index_title, t.col_tip_page) {
                        col_clicked = Some(0);
                    }
                });
                header.col(|ui| {
                    if header_cell(ui, key_title, t.col_tip_key) {
                        col_clicked = Some(1);
                    }
                });
                header.col(|ui| {
                    if header_cell(ui, &type_title, t.col_tip_page) {
                        col_clicked = Some(2);
                    }
                });
                header.col(|ui| {
                    if header_cell(ui, &value_title, t.col_tip_page) {
                        col_clicked = Some(3);
                    }
                });
            })
            .body(|body| {
                body.rows(text_height, total_rows, |mut row_ui| {
                    let di = row_ui.index();
                    let i = order[di];
                    let row = &app.rows[i];
                    let view = &app.views[i];
                    let is_sel = selected == Some(i);
                    row_ui.set_selected(is_sel);
                    // 文字保持默认色：选中行底色已改浅色，无需反白
                    // 单元格 Label 一律不带 click sense：hit-test 中可交互的 Label 会
                    // 抢走整行 cell 的 hover，导致"悬停文字无高亮、悬停空白才高亮"。
                    // 点击统一由行末 row_ui.response()（各 cell 响应的并集）承担。
                    row_ui.col(|ui| {
                        let abs = match app.base_index {
                            Some(b) => b + i + 1,
                            None => i + 1,
                        };
                        let rt = egui::RichText::new(abs.to_string());
                        // selectable(false)：避免单元格文本选区抢占 Ctrl+C，
                        // 保证 Ctrl+C 始终复制选中行的 KV
                        let resp = ui.add(
                            egui::Label::new(rt)
                                .selectable(false)
                                .show_tooltip_when_elided(false),
                        );
                        elide_tooltip(ui, resp, &abs.to_string());
                    });
                    row_ui.col(|ui| {
                        let rt = egui::RichText::new(&view.key_text).monospace();
                        let resp = ui.add(
                            egui::Label::new(rt)
                                .selectable(false)
                                .show_tooltip_when_elided(false),
                        );
                        cell_tooltip(
                            ui,
                            resp,
                            i,
                            &row.key,
                            &view.key_text,
                            view.key_clipped,
                            true,
                            app.key_mode,
                            app.endian,
                            app.hint_key,
                        );
                    });
                    row_ui.col(|ui| {
                        // Value 指定了解码类型时，类型标签用强调色并加 hover 说明，
                        // 与自动猜测的默认文本区分开
                        let forced = app.val_mode != crate::fmt::DecodeMode::Auto;
                        let mut rt = egui::RichText::new(&view.type_label);
                        if forced {
                            rt = rt.color(ui.visuals().hyperlink_color);
                        }
                        let resp = ui.add(
                            egui::Label::new(rt)
                                .selectable(false)
                                .show_tooltip_when_elided(false),
                        );
                        if forced {
                            // 标签被列宽截断时在 tooltip 里补全全名，再附指定类型说明
                            let font = ui.style().text_styles[&egui::TextStyle::Body].clone();
                            let label_w = ui.fonts_mut(|f| {
                                f.layout_no_wrap(
                                    view.type_label.clone(),
                                    font,
                                    egui::Color32::WHITE,
                                )
                                .size()
                                .x
                            });
                            let tip = if label_w > resp.rect.width() {
                                format!("{}\n\n{}", view.type_label, t.type_forced_tip)
                            } else {
                                t.type_forced_tip.to_string()
                            };
                            resp.on_hover_text(tip);
                        } else {
                            elide_tooltip(ui, resp, &view.type_label);
                        }
                    });
                    row_ui.col(|ui| {
                        // 多值表分组行：显示第一个值的预览 + 值总数（值列表在右侧翻看）
                        let text = match row.dup_count {
                            Some(n) => format!("{}{}", view.val_text, t.dup_n_values(n)),
                            None => view.val_text.clone(),
                        };
                        let rt = egui::RichText::new(&text).monospace();
                        let resp = ui.add(
                            egui::Label::new(rt)
                                .selectable(false)
                                .show_tooltip_when_elided(false),
                        );
                        cell_tooltip(
                            ui,
                            resp,
                            i,
                            &row.value,
                            &text,
                            view.val_clipped,
                            false,
                            app.val_mode,
                            app.endian,
                            app.hint_value,
                        );
                    });
                    if row_ui.response().clicked() {
                        clicked_row = Some(i);
                        app.row_copy_pending = true;
                    }
                });
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
