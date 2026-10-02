// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 右侧详情：Key / Value 卡片（格式下拉、复制、文本、hex dump、多值翻页），
//! 以及右栏全部状态（[`DetailState`]）。

use super::{MdbxerApp, Status, parse_bytes_input};
use crate::db;
use crate::fmt::{self, DecodeMode};
use crate::i18n::tr;
use libmdbx::{Database, NoWriteMap};

/// 右侧多值导航：每页懒加载的值个数。
pub const DUP_PAGE_SIZE: usize = 100;

/// 右侧详情面板状态：hex 视图配置、多值（DUP_SORT）导航、大字段分段查看。
pub struct DetailState {
    /// Key 卡片的解码格式
    pub key_mode: DecodeMode,
    /// Value 卡片的解码格式
    pub val_mode: DecodeMode,
    /// hex dump 是否显示地址列
    pub show_addr: bool,
    /// hex dump 是否显示十六进制列
    pub show_hex: bool,
    /// hex dump 是否显示 ASCII 列
    pub show_ascii: bool,
    /// hex dump 行宽（每行字节数：4/8/16/32）
    pub hex_width: usize,
    /// 多值表当前 Key 的值总数
    pub dup_total: usize,
    /// 当前选中值的全局序号（0 起）
    pub dup_index: usize,
    /// dup_values 中第一个值的全局序号（跨页加载用）
    pub dup_page_start: usize,
    /// 当前值页的数据（最多 DUP_PAGE_SIZE 个）
    pub dup_values: Vec<Vec<u8>>,
    /// 多值：序号跳转输入（1 起）
    pub dup_jump_input: String,
    /// 多值：值内容搜索输入（文本或 hex(...)）
    pub dup_search_input: String,
    /// Key 卡片当前段起始偏移
    pub key_seg_off: usize,
    /// Value 卡片当前段起始偏移
    pub val_seg_off: usize,
    /// Key 卡片偏移跳转输入框
    pub key_seg_input: String,
    /// Value 卡片偏移跳转输入框
    pub val_seg_input: String,
}

impl Default for DetailState {
    fn default() -> Self {
        Self {
            key_mode: DecodeMode::Auto,
            val_mode: DecodeMode::Auto,
            show_addr: true,
            show_hex: true,
            show_ascii: true,
            hex_width: fmt::DEFAULT_HEX_WIDTH,
            dup_total: 1,
            dup_index: 0,
            dup_page_start: 0,
            dup_values: Vec::new(),
            dup_jump_input: String::new(),
            dup_search_input: String::new(),
            key_seg_off: 0,
            val_seg_off: 0,
            key_seg_input: String::new(),
            val_seg_input: String::new(),
        }
    }
}

/// 多值/分段操作需要的数据库上下文：库句柄 + 表名（None = 主表）+ 当前 Key。
pub struct DupCtx<'a> {
    /// 数据库句柄（只读）
    pub db: &'a Database<NoWriteMap>,
    /// 表名；None = 主表
    pub table: Option<&'a str>,
    /// 当前选中行的 Key 字节
    pub key: &'a [u8],
}

impl DetailState {
    /// 清空多值与分段状态（换行/换表/关库时调用）。
    pub fn clear(&mut self) {
        self.dup_total = 1;
        self.dup_index = 0;
        self.dup_page_start = 0;
        self.dup_values.clear();
        self.dup_jump_input.clear();
        self.dup_search_input.clear();
        self.key_seg_off = 0;
        self.val_seg_off = 0;
        self.key_seg_input.clear();
        self.val_seg_input.clear();
    }

    /// 加载指定 Key 的多值首页（值列表每页 DUP_PAGE_SIZE 个懒加载）；
    /// 非多值表只清空状态。
    ///
    /// - `table`：None = 主表
    /// - `dup_sort`：该表是否为 DUP_SORT 多值表
    /// - `key`：选中行的 Key 字节
    pub fn load_dups(
        &mut self,
        db: &Database<NoWriteMap>,
        table: Option<&str>,
        dup_sort: bool,
        key: &[u8],
    ) {
        self.clear();
        if !dup_sort {
            return;
        }
        if let Ok((total, values)) = db::dups_of(db, table, key, 0, DUP_PAGE_SIZE) {
            self.dup_total = total.max(1);
            self.dup_values = values;
        }
    }

    /// 跳转到当前 Key 的第 `idx` 个值（0 起）；自动夹到有效范围，跨值页时懒加载。
    ///
    /// 返回 `Ok(())` 表示成功（或目标已在当前页，无需 IO）；
    /// 返回 `Err(msg)` 表示跨页加载失败，此时 `dup_index` 保持不变，
    /// 调用者应将 `msg` 显示到状态栏。
    pub fn dup_goto(&mut self, ctx: &DupCtx, idx: usize) -> Result<(), String> {
        if self.dup_total == 0 {
            return Ok(());
        }
        let idx = idx.min(self.dup_total - 1);
        let in_page =
            idx >= self.dup_page_start && idx < self.dup_page_start + self.dup_values.len();
        if !in_page {
            let page_index = idx / DUP_PAGE_SIZE;
            match db::dups_of(ctx.db, ctx.table, ctx.key, page_index, DUP_PAGE_SIZE) {
                Ok((total, values)) => {
                    self.dup_total = total.max(1);
                    self.dup_page_start = page_index * DUP_PAGE_SIZE;
                    self.dup_values = values;
                }
                Err(e) => return Err(tr().dup_load_fail(&e)),
            }
        }
        self.dup_index = idx.min(self.dup_total.saturating_sub(1));
        // 切换到另一个值：Value 分段偏移归零
        self.val_seg_off = 0;
        self.val_seg_input.clear();
        Ok(())
    }

    /// 上/下一个值（边界停止）。跨页加载失败时返回错误消息。
    pub fn dup_step(&mut self, ctx: &DupCtx, delta: isize) -> Result<(), String> {
        let new = self.dup_index as isize + delta;
        if new < 0 || new >= self.dup_total as isize {
            return Ok(());
        }
        self.dup_goto(ctx, new as usize)
    }

    /// 上/下翻一个值页（DUP_PAGE_SIZE 个值），到头自动夹住。跨页加载失败时返回错误消息。
    pub fn dup_page_step(&mut self, ctx: &DupCtx, pages: isize) -> Result<(), String> {
        let target = self.dup_index as isize + pages * DUP_PAGE_SIZE as isize;
        if target < 0 {
            self.dup_goto(ctx, 0)
        } else {
            self.dup_goto(ctx, target as usize)
        }
    }

    /// 序号跳转：输入为 1 起的十进制序号。返回状态栏消息。
    pub fn dup_jump(&mut self, ctx: &DupCtx) -> String {
        let t = tr();
        let s = self.dup_jump_input.trim();
        match s.parse::<usize>() {
            Ok(n) if n >= 1 && n <= self.dup_total => match self.dup_goto(ctx, n - 1) {
                Ok(()) => t.dup_located(n, self.dup_total),
                Err(e) => e,
            },
            Ok(n) => t.dup_range(n, self.dup_total),
            Err(_) => t.dup_bad_num.to_string(),
        }
    }

    /// 在当前 Key 的值中按内容搜索：文本按 UTF-8，hex(...)/0x... 按字节；字节子串匹配。
    /// `forward=false` 向小序号方向查找；主方向无命中时回绕。返回状态栏消息。
    pub fn dup_search(&mut self, ctx: &DupCtx, forward: bool) -> String {
        let t = tr();
        let s = self.dup_search_input.trim();
        if s.is_empty() {
            return t.dup_prompt.to_string();
        }
        let needle = match parse_bytes_input(s) {
            Ok(b) => b,
            Err(e) => return t.dup_bad_query(&e),
        };
        // 向后从下一个值开始；向前从当前值之前开始
        let from = if forward { self.dup_index + 1 } else { self.dup_index };
        match db::dup_find(ctx.db, ctx.table, ctx.key, &needle, from, forward) {
            Ok(Some((i, _))) => {
                let wrapped = forward && i < from || !forward && i >= from;
                match self.dup_goto(ctx, i) {
                    Ok(()) => {
                        if wrapped {
                            t.dup_wrap(i + 1, self.dup_total)
                        } else {
                            t.dup_located(i + 1, self.dup_total)
                        }
                    }
                    Err(e) => e,
                }
            }
            Ok(None) => t.dup_nomatch.to_string(),
            Err(e) => t.dup_search_fail(&e),
        }
    }

    /// Key/Value 卡片当前的解码格式（`is_key` 决定用 key_mode 还是 val_mode）。
    pub fn mode_of(&self, is_key: bool) -> DecodeMode {
        if is_key { self.key_mode } else { self.val_mode }
    }

    /// 写回 Key/Value 卡片的解码格式。
    pub fn set_mode(&mut self, is_key: bool, mode: DecodeMode) {
        if is_key {
            self.key_mode = mode;
        } else {
            self.val_mode = mode;
        }
    }

    /// Key/Value 卡片当前段偏移（`is_key=true` 取 Key，否则 Value）。
    pub fn seg_off(&self, is_key: bool) -> usize {
        if is_key { self.key_seg_off } else { self.val_seg_off }
    }

    /// Key/Value 卡片分段状态（偏移 + 跳转输入框）的可变引用。
    fn seg_state_mut(&mut self, is_key: bool) -> (&mut usize, &mut String) {
        if is_key {
            (&mut self.key_seg_off, &mut self.key_seg_input)
        } else {
            (&mut self.val_seg_off, &mut self.val_seg_input)
        }
    }

    /// 上/下翻 `pages` 个段（每段 fmt::PAGE_BYTES 字节），自动夹到有效范围。
    pub fn seg_step(&mut self, is_key: bool, total: usize, pages: isize) {
        let (off, _) = self.seg_state_mut(is_key);
        let cur = (*off / fmt::PAGE_BYTES) as isize;
        let max_seg = total.saturating_sub(1) / fmt::PAGE_BYTES;
        let target = (cur + pages).clamp(0, max_seg as isize) as usize;
        *off = target * fmt::PAGE_BYTES;
    }

    /// 跳至指定偏移：十进制或 0x 十六进制；向下对齐到段边界并夹到末尾段。
    /// 返回状态栏消息。
    pub fn seg_jump(&mut self, is_key: bool, total: usize) -> String {
        let t = tr();
        let (off, input) = self.seg_state_mut(is_key);
        let s = input.trim();
        let parsed = if let Some(h) = s
            .strip_prefix("0x")
            .or_else(|| s.strip_prefix("0X"))
        {
            usize::from_str_radix(h, 16)
        } else {
            s.parse::<usize>()
        };
        match parsed {
            Ok(v) if v < total => {
                *off = (v / fmt::PAGE_BYTES) * fmt::PAGE_BYTES;
                t.seg_ok(*off)
            }
            Ok(v) => t.seg_range_msg(v, total),
            Err(_) => t.seg_bad.to_string(),
        }
    }
}

/// 右栏入口：无选中行时显示提示；有选中行时显示 Key/Value 两张卡片。
pub fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let t = tr();
    let left_w = if app.left_visible {
        app.left_panel_w
    } else {
        0.0
    };
    let max_w = detail_max_width(ui, &app.detail, left_w);
    let resp = egui::Panel::right("detail_panel")
        .default_size(360.0)
        .size_range(240.0..=max_w)
        .show(ui, |ui| {
            let Some(row_idx) = app.selected_row else {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.weak(t.detail_select_hint);
                });
                return;
            };
            let Some(row) = app.rows.get(row_idx) else { return };
            let key = row.key.clone();
            let Some(value) = app.current_value() else { return };
            let key_no = app.base_index.map(|b| b + row_idx + 1);
            let dup_sort = app.cur_table().map(|tbl| tbl.dup_sort).unwrap_or(false);
            let (dup_index, dup_total) = (app.detail.dup_index, app.detail.dup_total);

            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(t.hex_view);
                    ui.checkbox(&mut app.detail.show_addr, t.addr);
                    ui.checkbox(&mut app.detail.show_hex, "HEX");
                    ui.checkbox(&mut app.detail.show_ascii, "ASCII");
                    ui.separator();
                    ui.label(t.width);
                    let mut w = app.detail.hex_width;
                    let ir = egui::ComboBox::from_id_salt("hex_width")
                        .selected_text(w.to_string())
                        .show_ui(ui, |ui| {
                            for v in fmt::HEX_WIDTHS {
                                ui.selectable_value(&mut w, v, v.to_string());
                            }
                        });
                    super::wheel_cycle(ui.ctx(), &ir.response, &fmt::HEX_WIDTHS, &mut w);
                    app.detail.hex_width = w;
                });
                ui.separator();

                let key_title = match key_no {
                    Some(n) => format!("Key #{n}"),
                    None => "Key".to_string(),
                };
                kv_card(ui, app, &key_title, &key, true, &key);
                ui.add_space(8.0);

                let val_title = if dup_sort {
                    t.val_title(dup_index + 1, dup_total)
                } else {
                    "Value".to_string()
                };
                kv_card(ui, app, &val_title, &value, false, &key);
            });
        });
    app.detail_panel_w = resp.response.rect.width();
}

/// 一个 Key 或 Value 卡片。`is_key` 决定使用 key_mode 还是 val_mode；
/// `key` 为选中行的 Key 字节（多值导航操作要用）。
fn kv_card(
    ui: &mut egui::Ui,
    app: &mut MdbxerApp,
    title: &str,
    bytes: &[u8],
    is_key: bool,
    key: &[u8],
) {
    let t = tr();
    let dup_sort = app.cur_table().map(|tbl| tbl.dup_sort).unwrap_or(false);
    let save_name = if is_key {
        "key.bin".to_string()
    } else if dup_sort {
        format!("value_{:06}.bin", app.detail.dup_index + 1)
    } else {
        "value.bin".to_string()
    };
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.horizontal(|ui| {
            ui.strong(title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t.copy).clicked() {
                    ui.ctx()
                        .copy_text(text_of(bytes, app.detail.mode_of(is_key), app.endian));
                }
                if ui
                    .button(t.save_as)
                    .on_hover_text(t.save_tip)
                    .clicked()
                {
                    app.save_bytes(&save_name, bytes);
                }
                // 格式下拉
                let mut mode = app.detail.mode_of(is_key);
                let ir = egui::ComboBox::from_id_salt(("detail_mode", is_key))
                    .selected_text(mode.label())
                    .height(430.0)
                    .show_ui(ui, |ui| {
                        for m in DecodeMode::ALL {
                            ui.selectable_value(&mut mode, m, m.label());
                        }
                    });
                super::wheel_cycle(ui.ctx(), &ir.response, &DecodeMode::ALL, &mut mode);
                app.detail.set_mode(is_key, mode);
            });
        });

        // 大字段分段：固定放在标题行正下方，避免随字节数/多值导航行数上下位移。
        // 每段 fmt::PAGE_BYTES 字节，超出时显示导航条。
        let total = bytes.len();
        let off = app.detail.seg_off(is_key);
        let off = if off >= total {
            total.saturating_sub(fmt::PAGE_BYTES.min(total))
        } else {
            off
        };
        if total > fmt::PAGE_BYTES {
            ui.horizontal(|ui| {
                // 按钮/输入框固定在最左：段号与偏移文本长度会变，放前面会挤动按钮
                if ui
                    .add_enabled(off > 0, egui::Button::new("◀"))
                    .on_hover_text(t.seg_prev_tip)
                    .clicked()
                {
                    app.detail.seg_step(is_key, total, -1);
                }
                if ui
                    .add_enabled(
                        off + fmt::PAGE_BYTES < total,
                        egui::Button::new("▶"),
                    )
                    .on_hover_text(t.seg_next_tip)
                    .clicked()
                {
                    app.detail.seg_step(is_key, total, 1);
                }
                let input = if is_key {
                    &mut app.detail.key_seg_input
                } else {
                    &mut app.detail.val_seg_input
                };
                let resp = ui.add(
                    egui::TextEdit::singleline(input)
                        .desired_width(84.0)
                        .hint_text(t.seg_input_hint),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    app.status = Status::Msg(app.detail.seg_jump(is_key, total));
                }
                let seg_no = off / fmt::PAGE_BYTES + 1;
                let seg_cnt = (total + fmt::PAGE_BYTES - 1) / fmt::PAGE_BYTES;
                ui.weak(t.seg_line(seg_no, seg_cnt, off, total));
            });
        }

        // 多值导航：仅 Value 卡片、多值表显示
        let is_dup = !is_key && app.cur_table().map(|tbl| tbl.dup_sort).unwrap_or(false);
        if is_dup {
            let (idx, total) = (app.detail.dup_index, app.detail.dup_total);
            let table_name = app.cur_table().and_then(|tbl| tbl.name.clone());
            if let Some(dbh) = app.db.as_ref() {
                let dctx = DupCtx {
                    db: &dbh.db,
                    table: table_name.as_deref(),
                    key,
                };
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(idx > 0, egui::Button::new("⏮"))
                        .on_hover_text(t.dup_tip_first)
                        .clicked()
                    {
                        if let Err(e) = app.detail.dup_goto(&dctx, 0) {
                            app.status = Status::Msg(e);
                        }
                    }
                    if ui
                        .add_enabled(idx > 0, egui::Button::new("⏪"))
                        .on_hover_text(t.dup_tip_prev100)
                        .clicked()
                    {
                        if let Err(e) = app.detail.dup_page_step(&dctx, -1) {
                            app.status = Status::Msg(e);
                        }
                    }
                    if ui
                        .add_enabled(idx > 0, egui::Button::new("◀"))
                        .on_hover_text(t.dup_tip_prev)
                        .clicked()
                    {
                        if let Err(e) = app.detail.dup_step(&dctx, -1) {
                            app.status = Status::Msg(e);
                        }
                    }
                    if ui
                        .add_enabled(idx + 1 < total, egui::Button::new("▶"))
                        .on_hover_text(t.dup_tip_next)
                        .clicked()
                    {
                        if let Err(e) = app.detail.dup_step(&dctx, 1) {
                            app.status = Status::Msg(e);
                        }
                    }
                    if ui
                        .add_enabled(idx + 1 < total, egui::Button::new("⏩"))
                        .on_hover_text(t.dup_tip_next100)
                        .clicked()
                    {
                        if let Err(e) = app.detail.dup_page_step(&dctx, 1) {
                            app.status = Status::Msg(e);
                        }
                    }
                    if ui
                        .add_enabled(idx + 1 < total, egui::Button::new("⏭"))
                        .on_hover_text(t.dup_tip_last)
                        .clicked()
                    {
                        if let Err(e) = app.detail.dup_goto(&dctx, total.saturating_sub(1)) {
                            app.status = Status::Msg(e);
                        }
                    }
                    ui.label(t.goto);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut app.detail.dup_jump_input)
                            .desired_width(48.0)
                            .hint_text("#"),
                    );
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        app.status = Status::Msg(app.detail.dup_jump(&dctx));
                    }
                });
                ui.horizontal(|ui| {
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut app.detail.dup_search_input)
                            .desired_width(178.0)
                            .hint_text(t.search_hint),
                    );
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui
                        .button("↑")
                        .on_hover_text(t.search_prev_tip)
                        .clicked()
                    {
                        app.status = Status::Msg(app.detail.dup_search(&dctx, false));
                    }
                    if ui
                        .button("↓")
                        .on_hover_text(t.search_next_tip)
                        .clicked()
                        || enter
                    {
                        app.status = Status::Msg(app.detail.dup_search(&dctx, true));
                    }
                });
                ui.add_space(2.0);
            }
        }

        // 自动模式时显示猜测的类型
        if app.detail.mode_of(is_key) == DecodeMode::Auto {
            ui.weak(t.guess_line(&fmt::guess(bytes, app.endian).0, bytes.len()));
        } else {
            ui.weak(t.bytes(bytes.len()));
        }

        // 分段窗口（导航条固定在卡片标题行正下方）
        let end = (off + fmt::PAGE_BYTES).min(total);
        let window = &bytes[off..end];

        // 文本视图（可折叠；长文本默认折叠，方便直接看 hex）。
        // 注意：egui 的 desired_rows 只设最小高度（内容少于该行数时撑开），
        // 不封顶；超出部分由外层右栏 ScrollArea 统一滚动。
        // 字符额度按最宽的 binary 展开（9 字符/字节）+ 64 余量，
        // 避免时间戳这类"小字节大文本"被截断出省略号。
        let text = fmt::decode(
            window,
            app.detail.mode_of(is_key),
            app.endian,
            window.len() * 9 + 64,
        );
        let text_rows = text.lines().count().clamp(1, 16);
        let default_open = total <= 512;
        egui::CollapsingHeader::new(t.section_text)
            .id_salt(("detail_text", is_key, default_open))
            .default_open(default_open)
            .show(ui, |ui| {
                let mut text = text;
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(text_rows),
                );
            });

        // 十六进制视图：desired_rows 只设最小高度（内容少时撑开），不封顶；
        // 整段行数多时由外层右栏 ScrollArea 统一滚动。
        let dump = fmt::hex_dump(
            window,
            app.detail.hex_width,
            app.detail.show_addr,
            app.detail.show_hex,
            app.detail.show_ascii,
            off,
        );
        let hex_rows = dump.lines().count().clamp(1, 20);
        egui::CollapsingHeader::new(t.section_hex)
            .id_salt(("detail_hex", is_key))
            .default_open(true)
            .show(ui, |ui| {
                let mut dump = dump;
                ui.add(
                    egui::TextEdit::multiline(&mut dump)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(hex_rows),
                );
            });
    });
}

/// 复制按钮用：完整解码文本（不受 cell_max 截断）。
fn text_of(bytes: &[u8], mode: DecodeMode, endian: fmt::Endian) -> String {
    fmt::decode(bytes, mode, endian, usize::MAX)
}

/// 右栏宽度上限：保证当前 hex 配置下最长一行（32 字节时最宽）
/// 在面板内不折行。按等宽字体实测字宽计算，再扣除各级边距；
/// 同时不超过窗口宽度减去左栏与中央表格最小保留宽度，
/// 窄窗口时下限放宽到右栏最小宽度 240，优先保证中央表格。
fn detail_max_width(ui: &egui::Ui, d: &DetailState, left_w: f32) -> f32 {
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font_id, '0'));

    let n = d.hex_width as f32;
    let mut line_chars = 0.0_f32;
    if d.show_addr {
        // 8 位十六进制地址 + 列后 2 空格
        line_chars += 10.0;
    }
    if d.show_hex {
        // 每字节 "XX "，宽行中间额外 1 个分隔空格
        line_chars += n * 3.0 + if d.hex_width >= 8 { 1.0 } else { 0.0 };
    }
    if d.show_ascii {
        line_chars += n;
    }
    // 面板边框/分组 frame/折叠缩进/文本框内边距与滚动条余量
    const CHROME: f32 = 100.0;
    let needed = line_chars * char_w + CHROME;

    let screen = ui.ctx().viewport_rect().width();
    needed
        .max(720.0)
        .min((screen - left_w - super::MIDDLE_MIN_WIDTH).max(240.0))
}
