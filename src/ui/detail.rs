// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 右侧详情：Key / Value 卡片（格式下拉、复制、文本、hex dump、多值翻页），
//! 以及右栏全部状态（[`DetailState`]）。

use super::{MdbxerApp, MsgLevel, Status, parse_bytes_input};
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
    /// Key 卡片大字段内容搜索输入（文本或 hex(...)）
    pub key_blob_input: String,
    /// Value 卡片大字段内容搜索输入
    pub val_blob_input: String,
    /// Key 卡片最近一次搜索命中区间（全局偏移 start, len），用于 hex 高亮
    pub key_blob_hit: Option<(usize, usize)>,
    /// Value 卡片最近一次搜索命中区间
    pub val_blob_hit: Option<(usize, usize)>,
    /// Key 卡片搜索命中序号与总数（1 起，VSCode 风格 n/m）
    pub key_blob_matches: Option<(usize, usize)>,
    /// Value 卡片搜索命中序号与总数
    pub val_blob_matches: Option<(usize, usize)>,
    /// Key 卡片待滚动的命中字节位置（全局偏移，消费一次后清空）
    pub key_blob_scroll: Option<usize>,
    /// Value 卡片待滚动的命中字节位置
    pub val_blob_scroll: Option<usize>,
    /// Key 卡片 hex 视图选区的内容指纹（段偏移/长度/行/多值序号），用于换内容时清选区
    pub hex_key_fp: Option<(usize, usize, Option<usize>, usize)>,
    /// Key 卡片 hex 视图选区（字节全局序号端点，顺序无关）
    pub hex_key_sel: Option<(usize, usize)>,
    /// Value 卡片 hex 视图选区内容指纹
    pub hex_val_fp: Option<(usize, usize, Option<usize>, usize)>,
    /// Value 卡片 hex 视图选区
    pub hex_val_sel: Option<(usize, usize)>,
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
            key_blob_input: String::new(),
            val_blob_input: String::new(),
            key_blob_hit: None,
            val_blob_hit: None,
            key_blob_matches: None,
            val_blob_matches: None,
            key_blob_scroll: None,
            val_blob_scroll: None,
            hex_key_fp: None,
            hex_key_sel: None,
            hex_val_fp: None,
            hex_val_sel: None,
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
    /// 详情卡片排版是"当前行"的临时查看选择，换行即回到自动；
    /// 同一 Key 内翻多值走 dup_goto 不调本函数，格式得以保持。
    pub fn clear(&mut self) {
        self.key_mode = DecodeMode::Auto;
        self.val_mode = DecodeMode::Auto;
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
        self.key_blob_input.clear();
        self.val_blob_input.clear();
        self.key_blob_hit = None;
        self.val_blob_hit = None;
        self.key_blob_matches = None;
        self.val_blob_matches = None;
        self.key_blob_scroll = None;
        self.val_blob_scroll = None;
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
        // 切换到另一个值：Value 分段偏移归零；旧值的搜索命中失效（输入保留）
        self.val_seg_off = 0;
        self.val_seg_input.clear();
        self.val_blob_hit = None;
        self.val_blob_matches = None;
        self.val_blob_scroll = None;
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

    /// 序号跳转：输入为 1 起的十进制序号。返回（级别, 状态栏消息）。
    pub fn dup_jump(&mut self, ctx: &DupCtx) -> (MsgLevel, String) {
        let t = tr();
        let s = self.dup_jump_input.trim();
        match s.parse::<usize>() {
            Ok(n) if n >= 1 && n <= self.dup_total => match self.dup_goto(ctx, n - 1) {
                Ok(()) => (MsgLevel::Info, t.dup_located(n, self.dup_total)),
                Err(e) => (MsgLevel::Error, e),
            },
            Ok(n) => (MsgLevel::Warn, t.dup_range(n, self.dup_total)),
            Err(_) => (MsgLevel::Warn, t.dup_bad_num.to_string()),
        }
    }

    /// 在当前 Key 的值中按内容搜索：文本按 UTF-8，hex(...)/0x... 按字节；字节子串匹配。
    /// `forward=false` 向小序号方向查找；主方向无命中时回绕。返回（级别, 状态栏消息）。
    pub fn dup_search(&mut self, ctx: &DupCtx, forward: bool) -> (MsgLevel, String) {
        let t = tr();
        let s = self.dup_search_input.trim();
        if s.is_empty() {
            return (MsgLevel::Warn, t.dup_prompt.to_string());
        }
        let needle = match parse_bytes_input(s) {
            Ok(b) => b,
            Err(e) => return (MsgLevel::Error, t.dup_bad_query(&e)),
        };
        // 向后从下一个值开始；向前从当前值之前开始
        let from = if forward {
            self.dup_index + 1
        } else {
            self.dup_index
        };
        match db::dup_find(ctx.db, ctx.table, ctx.key, &needle, from, forward) {
            Ok(Some((i, _))) => {
                let wrapped = forward && i < from || !forward && i >= from;
                match self.dup_goto(ctx, i) {
                    Ok(()) => {
                        if wrapped {
                            (MsgLevel::Info, t.dup_wrap(i + 1, self.dup_total))
                        } else {
                            (MsgLevel::Info, t.dup_located(i + 1, self.dup_total))
                        }
                    }
                    Err(e) => (MsgLevel::Error, e),
                }
            }
            Ok(None) => (MsgLevel::Warn, t.dup_nomatch.to_string()),
            Err(e) => (MsgLevel::Error, t.dup_search_fail(&e)),
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
        if is_key {
            self.key_seg_off
        } else {
            self.val_seg_off
        }
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
    /// 返回（级别, 状态栏消息）。
    pub fn seg_jump(&mut self, is_key: bool, total: usize) -> (MsgLevel, String) {
        let t = tr();
        let (off, input) = self.seg_state_mut(is_key);
        let s = input.trim();
        let parsed = if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            usize::from_str_radix(h, 16)
        } else {
            s.parse::<usize>()
        };
        match parsed {
            Ok(v) if v < total => {
                *off = (v / fmt::PAGE_BYTES) * fmt::PAGE_BYTES;
                (MsgLevel::Info, t.seg_ok(*off))
            }
            Ok(v) => (MsgLevel::Warn, t.seg_range_msg(v, total)),
            Err(_) => (MsgLevel::Warn, t.seg_bad.to_string()),
        }
    }

    /// Key/Value 卡片大字段搜索状态（输入框 + 命中区间 + 序号/总数）的可变引用。
    #[allow(clippy::type_complexity)]
    fn blob_state_mut(
        &mut self,
        is_key: bool,
    ) -> (
        &mut String,
        &mut Option<(usize, usize)>,
        &mut Option<(usize, usize)>,
    ) {
        if is_key {
            (
                &mut self.key_blob_input,
                &mut self.key_blob_hit,
                &mut self.key_blob_matches,
            )
        } else {
            (
                &mut self.val_blob_input,
                &mut self.val_blob_hit,
                &mut self.val_blob_matches,
            )
        }
    }

    /// Key/Value 卡片大字段搜索命中区间（只读，供 hex 高亮）。
    pub fn blob_hit(&self, is_key: bool) -> Option<(usize, usize)> {
        if is_key {
            self.key_blob_hit
        } else {
            self.val_blob_hit
        }
    }

    /// 取出并清空待滚动的命中字节位置（全局偏移）；内容区滚动组件消费一次。
    pub fn blob_scroll_take(&mut self, is_key: bool) -> Option<usize> {
        if is_key {
            self.key_blob_scroll.take()
        } else {
            self.val_blob_scroll.take()
        }
    }

    /// 只读待滚动的命中字节位置（不消费）：文本区先渲染、需要与 hex 区共享同一目标。
    pub fn blob_scroll(&self, is_key: bool) -> Option<usize> {
        if is_key {
            self.key_blob_scroll
        } else {
            self.val_blob_scroll
        }
    }

    /// Key/Value 卡片搜索命中序号与总数（只读，搜索行显示 n/m）。
    pub fn blob_matches(&self, is_key: bool) -> Option<(usize, usize)> {
        if is_key {
            self.key_blob_matches
        } else {
            self.val_blob_matches
        }
    }

    /// 在卡片完整字节中做字节子串搜索：文本按 UTF-8，`hex(...)/0x...` 按字节。
    /// `forward=false` 向小偏移方向；以当前命中为起点跳过自身，主方向无命中回绕。
    /// 命中后对齐到所在段并记录高亮区间。返回（级别, 状态栏消息）。
    pub fn blob_search(&mut self, is_key: bool, bytes: &[u8], forward: bool) -> (MsgLevel, String) {
        let t = tr();
        let s = self.blob_state_mut(is_key).0.trim().to_string();
        if s.is_empty() {
            *self.blob_state_mut(is_key).2 = None;
            return (MsgLevel::Warn, t.blob_prompt.to_string());
        }
        let needle = match parse_bytes_input(&s) {
            Ok(b) => b,
            Err(e) => return (MsgLevel::Error, t.dup_bad_query(&e)),
        };
        if needle.is_empty() || needle.len() > bytes.len() {
            *self.blob_state_mut(is_key).2 = None;
            return (MsgLevel::Warn, t.blob_nomatch.to_string());
        }
        let cur = self.blob_hit(is_key);
        let (pos, wrapped) = if forward {
            // 从当前命中末尾之后继续；无命中则从头搜
            let start = cur.map(|(p, l)| p + l).unwrap_or(0).min(bytes.len());
            match bytes[start..]
                .windows(needle.len())
                .position(|w| w == needle)
                .map(|i| start + i)
            {
                Some(p) => (p, false),
                None if start > 0 => {
                    // 回绕：在 [0, start) 内找
                    match bytes[..start]
                        .windows(needle.len())
                        .position(|w| w == needle)
                    {
                        Some(p) => (p, true),
                        None => {
                            *self.blob_state_mut(is_key).2 = None;
                            return (MsgLevel::Warn, t.blob_nomatch.to_string());
                        }
                    }
                }
                None => {
                    *self.blob_state_mut(is_key).2 = None;
                    return (MsgLevel::Warn, t.blob_nomatch.to_string());
                }
            }
        } else {
            // 从当前命中起点之前找；无命中则从末尾搜
            let start = cur.map(|(p, _)| p).unwrap_or(bytes.len()).min(bytes.len());
            match bytes[..start]
                .windows(needle.len())
                .rposition(|w| w == needle)
            {
                Some(p) => (p, false),
                None if start < bytes.len() => {
                    match bytes[start..]
                        .windows(needle.len())
                        .rposition(|w| w == needle)
                        .map(|i| start + i)
                    {
                        Some(p) => (p, true),
                        None => {
                            *self.blob_state_mut(is_key).2 = None;
                            return (MsgLevel::Warn, t.blob_nomatch.to_string());
                        }
                    }
                }
                None => {
                    *self.blob_state_mut(is_key).2 = None;
                    return (MsgLevel::Warn, t.blob_nomatch.to_string());
                }
            }
        };
        // 统计全部命中（不重叠）与当前命中序号，供搜索行显示 VSCode 风格 n/m
        let mut total = 0usize;
        let mut cur_no = 0usize;
        let mut scan = 0usize;
        while scan + needle.len() <= bytes.len() {
            match bytes[scan..].windows(needle.len()).position(|w| w == needle) {
                Some(rel) => {
                    let p = scan + rel;
                    total += 1;
                    if p == pos {
                        cur_no = total;
                    }
                    scan = p + needle.len();
                }
                None => break,
            }
        }
        // 记录命中并跳到所在段；置滚动目标，内容区滚动组件本帧消费后把命中行滚到可见
        let (_, hit, matches) = self.blob_state_mut(is_key);
        *hit = Some((pos, needle.len()));
        *matches = Some((cur_no.max(1), total));
        let off = self.seg_state_mut(is_key).0;
        *off = (pos / fmt::PAGE_BYTES) * fmt::PAGE_BYTES;
        if is_key {
            self.key_blob_scroll = Some(pos);
        } else {
            self.val_blob_scroll = Some(pos);
        }
        if wrapped {
            (MsgLevel::Info, t.blob_wrap(pos))
        } else {
            (MsgLevel::Info, t.blob_located(pos))
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
    // 行宽/列开关变化后按新配置内容宽度自动收放右栏（面板本体只会被超宽内容
    // 撑到上限，内容变窄时不会自动缩回，需在 show 结束后改写 PanelState）
    let mut hex_pref_changed = false;
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
            let Some(row) = app.rows.get(row_idx) else {
                return;
            };
            let key = row.key.clone();
            let Some(value) = app.current_value() else {
                return;
            };
            let key_no = app.base_index.map(|b| b + row_idx + 1);
            let dup_sort = app.cur_table().map(|tbl| tbl.dup_sort).unwrap_or(false);
            let (dup_index, dup_total) = (app.detail.dup_index, app.detail.dup_total);

            // 单行紧凑：三个列开关 + 行宽下拉，文字说明全部走 hover 提示
            ui.horizontal(|ui| {
                // HEX 与 ASCII 至少保留一项：只剩一项时该项变灰、不可取消；
                // 即只有另一项仍勾选时，才允许关掉这一项。
                hex_pref_changed = ui
                    .checkbox(&mut app.detail.show_addr, t.addr)
                    .on_hover_text(t.hex_addr_tip)
                    .changed()
                    | ui
                        .add_enabled(
                            app.detail.show_ascii,
                            egui::Checkbox::new(&mut app.detail.show_hex, "HEX"),
                        )
                        .on_hover_text(t.hex_col_tip)
                        .changed()
                    | ui
                        .add_enabled(
                            app.detail.show_hex,
                            egui::Checkbox::new(&mut app.detail.show_ascii, "ASCII"),
                        )
                        .on_hover_text(t.hex_ascii_tip)
                        .changed();
                ui.separator();
                let mut w = app.detail.hex_width;
                let ir = egui::ComboBox::from_id_salt("hex_width")
                    .width(52.0)
                    .selected_text(w.to_string())
                    .show_ui(ui, |ui| {
                        for v in fmt::HEX_WIDTHS {
                            ui.selectable_value(&mut w, v, v.to_string());
                        }
                    });
                super::wheel_cycle(ui.ctx(), &ir.response, &fmt::HEX_WIDTHS, &mut w);
                ir.response.on_hover_text(t.hex_width_tip);
                if w != app.detail.hex_width {
                    app.detail.hex_width = w;
                    hex_pref_changed = true;
                }
                if hex_pref_changed {
                    app.save_ui_prefs();
                }
            });
            ui.separator();

            // 预取两卡片的分段窗口与解码文本，用于估算自然高度
            let (_, k_win) = card_window(app, &key, true);
            let k_text = fmt::decode(
                k_win,
                app.detail.key_mode,
                app.endian,
                k_win.len() * 9 + 64,
            );
            let (_, v_win) = card_window(app, &value, false);
            let v_text = fmt::decode(
                v_win,
                app.detail.val_mode,
                app.endian,
                v_win.len() * 9 + 64,
            );

            // 高度预算：两张卡片都装得下时各按自然高度；装不下时至少均分——
            // 小卡片按自然高度渲染，省下的余量全部让给大卡片。
            // 预算在进入外层滚动区之前量好（滚动区内 available_height 是"无限"）。
            let avail = ui.available_height();
            let nk = card_natural_height(ui, app, key.len(), k_win, &k_text, true, false);
            let nv = card_natural_height(ui, app, value.len(), v_win, &v_text, false, dup_sort);
            let gap = 8.0;
            let half = (avail - gap) / 2.0;
            // Value 卡片的预算在 Key 卡片渲染完后按实测高度回填（见下），这里只需 Key 份额
            let (kb, _vb) = if nk + gap + nv <= avail {
                (nk, nv)
            } else if nk <= half {
                (nk, avail - gap - nk)
            } else if nv <= half {
                (avail - gap - nv, nv)
            } else {
                (half, half)
            };

            // 外层滚动区仅作兜底：预算准确时不出现；估算偏差溢出时仍能滚到
            egui::ScrollArea::vertical().show(ui, |ui| {
                let key_title = match key_no {
                    Some(n) => format!("Key #{n}"),
                    None => "Key".to_string(),
                };
                let kh = kv_card(ui, app, &key_title, &key, true, &key, &k_text, kb);
                ui.add_space(gap);

                // Key 卡片没用完的预算（估算偏差/内容少）全部实测回填给 Value，
                // 内容丰富时两卡恰好撑满右栏，底部不留空白
                let vb = (avail - gap - kh).max(120.0);
                let val_title = if dup_sort {
                    t.val_title(dup_index + 1, dup_total)
                } else {
                    "Value".to_string()
                };
                kv_card(ui, app, &val_title, &value, false, &key, &v_text, vb);
            });
        });
    app.detail_panel_w = resp.response.rect.width();
    if hex_pref_changed {
        let limit = (ui.ctx().viewport_rect().width() - left_w - super::MIDDLE_MIN_WIDTH)
            .max(240.0);
        let target = detail_needed_width(ui, &app.detail).clamp(240.0, limit);
        let rect =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(target, 0.0));
        ui.ctx().data_mut(|d| {
            d.insert_persisted(
                egui::Id::new("detail_panel"),
                egui::PanelState { outer_rect: rect },
            )
        });
    }
}

/// 卡片当前段窗口（偏移自动夹到有效范围），供高度估算与渲染共用。
fn card_window<'b>(app: &MdbxerApp, bytes: &'b [u8], is_key: bool) -> (usize, &'b [u8]) {
    let total = bytes.len();
    let off = app.detail.seg_off(is_key);
    let off = if off >= total {
        total.saturating_sub(fmt::PAGE_BYTES.min(total))
    } else {
        off
    };
    let end = (off + fmt::PAGE_BYTES).min(total);
    (off, &bytes[off..end])
}

/// 卡片文本/hex 两区的折叠状态。估算与渲染必须使用同一组 Id，
/// 这里用全局 `Id::new`（不用 `ui.make_persistent_id`，否则会带上
/// ScrollArea/分组的路径前缀，show() 里预估算时对不上）。
fn card_section_open(ctx: &egui::Context, is_key: bool) -> (bool, bool) {
    // 文本区默认展开：卡片高度已自适应（内部 ScrollArea + 16 行上限），
    // 大值折叠反而要多点一次；Id 盐值随之固定（不再随大小改变默认值）
    let text_open = egui::collapsing_header::CollapsingState::load_with_default_open(
        ctx,
        egui::Id::new(("detail_text", is_key, true)),
        true,
    )
    .is_open();
    let hex_open = egui::collapsing_header::CollapsingState::load_with_default_open(
        ctx,
        egui::Id::new(("detail_hex", is_key)),
        true,
    )
    .is_open();
    (text_open, hex_open)
}

/// 卡片自然高度估算：固定开销（标题/导航/搜索/两个折叠头/间距）
/// + 文本区（≤16 行）+ hex 区（行数 × 行高）。折叠的区按 0 计，
/// 高度让给其它区（折叠一方后可用高度理应更多）。
fn card_natural_height(
    ui: &egui::Ui,
    app: &MdbxerApp,
    total: usize,
    window: &[u8],
    text: &str,
    is_key: bool,
    dup_nav: bool,
) -> f32 {
    let line_h = ui
        .fonts_mut(|f| f.row_height(&egui::TextStyle::Monospace.resolve(ui.style())))
        + ui.spacing().extra_text_line_spacing;
    let fixed = card_fixed_overhead(total, is_key, dup_nav);
    let (text_open, hex_open) = card_section_open(ui.ctx(), is_key);
    let (text_h, hex_h) =
        card_content_heights(ui, app, window, text, line_h, f32::INFINITY, text_open, hex_open);
    fixed + text_h + hex_h
}

/// 卡片固定开销：标题行 + 两个折叠标题行 + 可选的分段/搜索/多值导航行 + 间距。
fn card_fixed_overhead(total: usize, _is_key: bool, dup_nav: bool) -> f32 {
    let mut h = 26.0 + 24.0 * 2.0 + 20.0; // 标题 + 文本头 + hex 头 + 间距
    if total > fmt::PAGE_BYTES {
        h += 28.0; // 分段导航行
    }
    if total > 512 {
        h += 28.0; // 字段内搜索行
    }
    if dup_nav {
        h += 56.0; // 多值导航两行
    }
    h
}

/// 文本/hex 内容区在内容预算 `content_budget` 下的实际高度分配：
/// 都装得下按自然高度；装不下时至少各分一半，小的一方把余量让给大的一方；
/// 折叠的区分 0、预算全部给展开的一方。
fn card_content_heights(
    ui: &egui::Ui,
    app: &MdbxerApp,
    window: &[u8],
    text: &str,
    line_h: f32,
    content_budget: f32,
    text_open: bool,
    hex_open: bool,
) -> (f32, f32) {
    // 文本自然高度按真实折行计算（与 TextEdit 渲染一致，防止长逻辑行被估成 1 行），
    // 上限 16 行；galley 由 egui 全局缓存，重复估算开销可忽略
    let text_nat = if text_open {
        let font_id = egui::TextStyle::Monospace.resolve(ui.style());
        let wrap_w = (ui.available_width() - 40.0).max(80.0); // 扣折叠缩进/滚动条/文本框内边距
        let galley = ui
            .fonts_mut(|f| f.layout(text.to_string(), font_id, egui::Color32::WHITE, wrap_w));
        galley.size().y.max(line_h).min(16.0 * line_h) + 12.0
    } else {
        0.0
    };
    let hex_rows = window.len().div_ceil(app.detail.hex_width).max(1);
    let hex_nat = if hex_open {
        hex_rows as f32 * line_h + 12.0
    } else {
        0.0
    };
    if text_nat + hex_nat <= content_budget {
        return (text_nat, hex_nat);
    }
    let half = content_budget / 2.0;
    if text_nat <= half {
        (text_nat, content_budget - text_nat)
    } else if hex_nat <= half {
        (content_budget - hex_nat, hex_nat)
    } else {
        (half, half)
    }
}

/// 一个 Key 或 Value 卡片。`is_key` 决定使用 key_mode 还是 val_mode；
/// `key` 为选中行的 Key 字节（多值导航操作要用）；
/// `text` 为当前段窗口已解码的文本；`budget` 为本卡片可用高度上限
/// （超过时文本/hex 两区均分余量、各自内部滚动）。
fn kv_card(
    ui: &mut egui::Ui,
    app: &mut MdbxerApp,
    title: &str,
    bytes: &[u8],
    is_key: bool,
    key: &[u8],
    text: &str,
    budget: f32,
) -> f32 {
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
        // 卡片内容起始游标：hex 区高度用"预算 − 已用"实测回填，
        // 不靠前文估算，保证卡片始终撑满分到的预算
        let card_top = ui.cursor().min.y;

        ui.horizontal(|ui| {
            ui.strong(title);
            // Key 卡片：收藏 ☆/★（点击切换，立即持久化）
            if is_key {
                let is_fav = app.is_fav_key(key);
                let star = if is_fav { "★" } else { "☆" };
                let star_tip = if is_fav {
                    t.fav_rm_k_tip
                } else {
                    t.fav_add_k_tip
                };
                if ui.small_button(star).on_hover_text(star_tip).clicked() {
                    app.toggle_fav_key(key);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t.copy).clicked() {
                    ui.ctx()
                        .copy_text(text_of(bytes, app.detail.mode_of(is_key), app.endian));
                }
                if ui.button(t.save_as).on_hover_text(t.save_tip).clicked() {
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
                // 详情卡片排版独立于顶栏表格排版：只改本卡片，不回写全局、不存偏好。
                if app.detail.mode_of(is_key) != mode {
                    app.detail.set_mode(is_key, mode);
                }
            });
        });

        // 大字段分段：固定放在标题行正下方，避免随字节数/多值导航行数上下位移。
        // 每段 fmt::PAGE_BYTES 字节，超出时显示导航条。
        let total = bytes.len();
        let (off, window) = card_window(app, bytes, is_key);
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
                    .add_enabled(off + fmt::PAGE_BYTES < total, egui::Button::new("▶"))
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
                    app.status = Status::leveled(app.detail.seg_jump(is_key, total));
                }
                let seg_no = off / fmt::PAGE_BYTES + 1;
                let seg_cnt = (total + fmt::PAGE_BYTES - 1) / fmt::PAGE_BYTES;
                ui.weak(t.seg_line(seg_no, seg_cnt, off, total));
            });
        }

        // 大字段内容搜索：较长内容才显示（文本按 UTF-8，hex(...)/0x... 按字节，回绕）
        if total > 512 {
            ui.horizontal(|ui| {
                let input = if is_key {
                    &mut app.detail.key_blob_input
                } else {
                    &mut app.detail.val_blob_input
                };
                let resp = ui.add(
                    egui::TextEdit::singleline(input)
                        .desired_width(178.0)
                        .hint_text(t.search_hint),
                );
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let (ctrl, shift) =
                    ui.input(|i| (i.modifiers.ctrl, i.modifiers.shift));
                // Enter=下一个；Shift+Enter 或 Ctrl+Enter=上一个
                let prev = ui.button("⬆").on_hover_text(t.search_prev_tip).clicked()
                    || (enter && (shift || ctrl));
                let next =
                    ui.button("⬇").on_hover_text(t.search_next_tip).clicked() || (enter && !shift && !ctrl);
                if prev || next {
                    app.last_search = if is_key {
                        super::SearchContext::DetailKeyBlob
                    } else {
                        super::SearchContext::DetailValBlob
                    };
                }
                if prev {
                    app.status = Status::leveled(app.detail.blob_search(is_key, bytes, false));
                }
                if next {
                    app.status = Status::leveled(app.detail.blob_search(is_key, bytes, true));
                }
                // egui singleline 回车默认 surrender_focus，焦点离开后连续回车
                // 无法继续搜下一个命中——搜索完立刻把焦点要回此输入框
                if enter {
                    resp.request_focus();
                }
                // VSCode 风格命中计数：当前第 n 个 / 共 m 个
                if let Some((n, m)) = app.detail.blob_matches(is_key) {
                    ui.weak(format!("{n}/{m}"));
                }
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
                            app.status = Status::error(e);
                        }
                    }
                    if ui
                        .add_enabled(idx > 0, egui::Button::new("⏪"))
                        .on_hover_text(t.dup_tip_prev100)
                        .clicked()
                    {
                        if let Err(e) = app.detail.dup_page_step(&dctx, -1) {
                            app.status = Status::error(e);
                        }
                    }
                    if ui
                        .add_enabled(idx > 0, egui::Button::new("◀"))
                        .on_hover_text(t.dup_tip_prev)
                        .clicked()
                    {
                        if let Err(e) = app.detail.dup_step(&dctx, -1) {
                            app.status = Status::error(e);
                        }
                    }
                    if ui
                        .add_enabled(idx + 1 < total, egui::Button::new("▶"))
                        .on_hover_text(t.dup_tip_next)
                        .clicked()
                    {
                        if let Err(e) = app.detail.dup_step(&dctx, 1) {
                            app.status = Status::error(e);
                        }
                    }
                    if ui
                        .add_enabled(idx + 1 < total, egui::Button::new("⏩"))
                        .on_hover_text(t.dup_tip_next100)
                        .clicked()
                    {
                        if let Err(e) = app.detail.dup_page_step(&dctx, 1) {
                            app.status = Status::error(e);
                        }
                    }
                    if ui
                        .add_enabled(idx + 1 < total, egui::Button::new("⏭"))
                        .on_hover_text(t.dup_tip_last)
                        .clicked()
                    {
                        if let Err(e) = app.detail.dup_goto(&dctx, total.saturating_sub(1)) {
                            app.status = Status::error(e);
                        }
                    }
                    ui.label(t.goto);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut app.detail.dup_jump_input)
                            .desired_width(48.0)
                            .hint_text("#"),
                    );
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        app.status = Status::leveled(app.detail.dup_jump(&dctx));
                    }
                });
                ui.horizontal(|ui| {
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut app.detail.dup_search_input)
                            .desired_width(178.0)
                            .hint_text(t.search_hint),
                    );
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let (ctrl, shift) =
                        ui.input(|i| (i.modifiers.ctrl, i.modifiers.shift));
                    // Enter=下一个；Shift+Enter 或 Ctrl+Enter=上一个
                    let prev = ui.button("⬆").on_hover_text(t.search_prev_tip).clicked()
                        || (enter && (shift || ctrl));
                    let next = ui.button("⬇").on_hover_text(t.search_next_tip).clicked()
                        || (enter && !shift && !ctrl);
                    if prev || next {
                        app.last_search = super::SearchContext::DetailDup;
                    }
                    if prev {
                        app.status = Status::leveled(app.detail.dup_search(&dctx, false));
                    }
                    if next {
                        app.status = Status::leveled(app.detail.dup_search(&dctx, true));
                    }
                    // 回车后焦点保留在搜索框，连续 Enter 可逐个命中
                    if enter {
                        resp.request_focus();
                    }
                });
                ui.add_space(2.0);
            }
        }

        // 文本视图（可折叠；默认展开，大值靠内部 ScrollArea + 16 行上限约束高度）。
        // egui TextEdit 自身无内部滚动、高度随内容无限增长（desired_rows 只是下限），
        // 故必须包一层 ScrollArea。内容预算在文本/hex 两区均分卡片预算的余量，
        // 装不下时各自内部滚动，卡片头/搜索行不随内容滚走。
        let line_h = ui
            .fonts_mut(|f| f.row_height(&egui::TextStyle::Monospace.resolve(ui.style())))
            + ui.spacing().extra_text_line_spacing;
        let fixed = card_fixed_overhead(total, is_key, is_dup);
        let (text_open, hex_open) = card_section_open(ui.ctx(), is_key);
        // hex 份额不在这里定：渲染时按游标实测回填（见下），此处只分文本区
        let (text_h, _) = card_content_heights(
            ui,
            app,
            window,
            text,
            line_h,
            (budget - fixed).max(80.0),
            text_open,
            hex_open,
        );
        // 猜测/字节数信息紧跟"文本"标题文字同一行右侧（自定义标题行），
        // 不再单列一行占据文本框与 hex 之间的位置
        let guess_text = if app.detail.mode_of(is_key) == DecodeMode::Auto {
            t.guess_line(&fmt::guess(bytes, app.endian).0, bytes.len())
        } else {
            t.bytes(bytes.len())
        };
        // 搜索命中联动：文本区也滚到命中附近。hex 展开时只看不取（留给 hex 区消费），
        // hex 折叠时由文本区消费，避免滚动目标滞留导致每帧抢滚动
        let text_scroll_to = if hex_open {
            app.detail.blob_scroll(is_key)
        } else {
            app.detail.blob_scroll_take(is_key)
        };
        // 文本区命中高亮区间（文本字节坐标）：仅当窗口是合法 UTF-8 且解码长度一致
        // （解码即原文，字节偏移 1:1）时可靠；hex/数值等重编码显示不映射，只滚动不高亮
        let text_hit_range: Option<(usize, usize)> = (|| {
            let (hit_abs, hit_len) = app.detail.blob_hit(is_key)?;
            if hit_abs < off || hit_abs >= off + window.len() {
                return None;
            }
            if text.len() != window.len() || std::str::from_utf8(window).is_err() {
                return None;
            }
            let mut hs = hit_abs - off;
            let mut he = (hs + hit_len).min(text.len());
            while hs > 0 && !text.is_char_boundary(hs) {
                hs -= 1;
            }
            while he < text.len() && !text.is_char_boundary(he) {
                he += 1;
            }
            (hs < he).then_some((hs, he))
        })();
        egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            egui::Id::new(("detail_text", is_key, true)),
            true,
        )
        .show_header(ui, |ui| {
            // 与 CollapsingHeader 默认一致：标题用 Button 样式文本
            ui.add(
                egui::Label::new(
                    egui::RichText::new(t.section_text).text_style(egui::TextStyle::Button),
                )
                .selectable(false),
            );
            ui.weak(guess_text);
        })
        .body(|ui| {
            let mut text_owned = text.to_string();
            // + 12 ≈ TextEdit frame 上下内边距，保证整数行内容恰好不滚
            egui::ScrollArea::vertical()
                .id_salt(("detail_text_scroll", is_key))
                .auto_shrink([false, true])
                .max_height(text_h)
                .show(ui, |ui| {
                    let content_top = ui.cursor().min.y;
                    let mut te = egui::TextEdit::multiline(&mut text_owned)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(1);
                    // 命中区间琥珀色高亮（与 hex 视图同色半透明，深浅主题均可读）
                    // 闭包先绑定变量再取 &mut，否则临时量在 add(te) 前就被释放（E0716）
                    // 必须与 egui 默认 multiline layouter 行为一致：wrap 宽度生效折行、
                    // 逐段设行高、保留行尾空白——否则单行长内容不折行，文本区只剩一截
                    // 横向被裁的"一行"，看起来就像详情内容消失
                    let mut hl_layouter =
                        move |ui: &egui::Ui, s: &dyn egui::TextBuffer, wrap: f32| {
                            let s = s.as_str();
                            let font_id = egui::TextStyle::Monospace.resolve(ui.style());
                            let line_height = ui.fonts_mut(|f| f.row_height(&font_id))
                                + ui.spacing().extra_text_line_spacing;
                            let base = egui::text::TextFormat {
                                font_id,
                                color: ui.visuals().text_color(),
                                line_height: Some(line_height),
                                ..Default::default()
                            };
                            let mut job = egui::text::LayoutJob::default();
                            if let Some((hs, he)) = text_hit_range {
                                job.append(&s[..hs], 0.0, base.clone());
                                job.append(
                                    &s[hs..he],
                                    0.0,
                                    egui::text::TextFormat {
                                        background: egui::Color32::from_rgba_unmultiplied(
                                            0xF5, 0xA6, 0x23, 0x66,
                                        ),
                                        ..base.clone()
                                    },
                                );
                                job.append(&s[he..], 0.0, base);
                            } else {
                                job.append(s, 0.0, base);
                            }
                            job.wrap.max_width = wrap;
                            job.keep_trailing_whitespace = true;
                            ui.fonts_mut(|f| f.layout_job(job))
                        };
                    if text_hit_range.is_some() {
                        te = te.layouter(&mut hl_layouter);
                    }
                    let te_rect = ui.add(te).rect;
                    // 搜索命中：滚到命中所在行（UTF-8 窗口字节偏移直接可用，
                    // 其它编码按字节比例估算后居中，误差不可见）
                    if let Some(hit_abs) = text_scroll_to {
                        if hit_abs >= off && hit_abs < off + window.len() {
                            let hit_rel = hit_abs - off;
                            let mut bp = (if std::str::from_utf8(window).is_ok() {
                                hit_rel
                            } else {
                                text.len() * hit_rel / window.len().max(1)
                            })
                            .min(text.len());
                            while bp > 0 && !text.is_char_boundary(bp) {
                                bp -= 1;
                            }
                            let font_id = egui::TextStyle::Monospace.resolve(ui.style());
                            let wrap_w = (te_rect.width() - 16.0).max(40.0);
                            let galley = ui.fonts_mut(|f| {
                                f.layout(text.to_string(), font_id, egui::Color32::WHITE, wrap_w)
                            });
                            let pc = text[..bp].chars().count();
                            // 按行累计字形数定位命中行（每字形 ≈ 一字符；
                            // 居中滚动容忍近似，egui 0.36 无 from_ccursor）
                            let mut acc = 0usize;
                            let mut row_y =
                                galley.rows.last().map(|r| r.rect().top()).unwrap_or(0.0);
                            for row in &galley.rows {
                                if acc + row.glyphs.len() > pc {
                                    row_y = row.rect().top();
                                    break;
                                }
                                acc += row.glyphs.len();
                            }
                            let rect = egui::Rect::from_min_size(
                                egui::pos2(te_rect.left(), content_top + 8.0 + row_y),
                                egui::vec2(4.0, line_h),
                            );
                            ui.scroll_to_rect(rect, Some(egui::Align::Center));
                        }
                    }
                });
        });

        // 十六进制视图：自绘交互组件（悬停整行/单字节联动、拖拽选区 HEX↔ASCII 同步）。
        // 独立滚动区：高度随内容自适应（auto_shrink 高度方向），只有超过上限才出滚动条；
        // 卡片头/搜索行固定在外面，滚动中也能继续搜索；命中后自动滚到可视区。
        // 用 CollapsingState + 全局 Id（与 card_section_open 预算估算同一组），
        // 折叠状态变化时高度分配下一帧即自适应。
        // hex 区高度实测回填：吃掉卡片预算减去已用高度（44 ≈ hex 标题行
        // + 分组 frame 下内边距 + 尾部间距），卡片始终撑满、右栏底部不留空白。
        let hex_h = (budget - (ui.cursor().min.y - card_top) - 44.0).max(line_h * 2.0);
        egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            egui::Id::new(("detail_hex", is_key)),
            true,
        )
        .show_header(ui, |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(t.section_hex).text_style(egui::TextStyle::Button),
                )
                .selectable(false),
            );
        })
        .body(|ui| {
                // 搜索命中先取（避免与下面 sel 的可变借用冲突）
                let blob_hit = app.detail.blob_hit(is_key);
                let scroll_to = app.detail.blob_scroll_take(is_key);
                // 内容指纹：换行/翻多值/切段后字节变了，选区作废
                let fp = (off, window.len(), app.selected_row, app.detail.dup_index);
                let (stored, sel) = if is_key {
                    (&mut app.detail.hex_key_fp, &mut app.detail.hex_key_sel)
                } else {
                    (&mut app.detail.hex_val_fp, &mut app.detail.hex_val_sel)
                };
                if *stored != Some(fp) {
                    *sel = None;
                }
                egui::ScrollArea::vertical()
                    .id_salt(("detail_hex_scroll", is_key))
                    // 宽度填满、高度随内容收缩；超过卡片分到的预算才出现滚动条
                    .auto_shrink([false, true])
                    .max_height(hex_h)
                    .show(ui, |ui| {
                        super::hexview::hex_view(
                            ui,
                            super::hexview::view_id(is_key),
                            window,
                            app.detail.hex_width,
                            app.detail.show_addr,
                            app.detail.show_hex,
                            app.detail.show_ascii,
                            off,
                            sel,
                            blob_hit,
                            scroll_to,
                        );
                    });
                *stored = Some(fp);
            });
        })
        .response
        .rect
        .height()
}

/// 复制按钮用：完整解码文本（不受 cell_max 截断）。
fn text_of(bytes: &[u8], mode: DecodeMode, endian: fmt::Endian) -> String {
    fmt::decode(bytes, mode, endian, usize::MAX)
}

/// 右栏宽度上限：保证当前 hex 配置下最长一行（16 字节时最宽）
/// 在面板内不折行。按等宽字体实测字宽计算，再扣除各级边距；
/// 同时不超过窗口宽度减去左栏与中央表格最小保留宽度，
/// 窄窗口时下限放宽到右栏最小宽度 240，优先保证中央表格。
fn detail_max_width(ui: &egui::Ui, d: &DetailState, left_w: f32) -> f32 {
    let needed = detail_needed_width(ui, d);
    let screen = ui.ctx().viewport_rect().width();
    needed
        .max(720.0)
        .min((screen - left_w - super::MIDDLE_MIN_WIDTH).max(240.0))
}

/// 当前 hex 配置（行宽 + 地址/HEX/ASCII 开关）下内容实际所需宽度。
fn detail_needed_width(ui: &egui::Ui, d: &DetailState) -> f32 {
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
    // 面板边框/分组 frame/折叠缩进/滚动条余量（实测约 60~70，留少量冗余；
    // 估小了内容超宽会把面板顶回去，估大了右侧留空白）
    const CHROME: f32 = 80.0;
    line_chars * char_w + CHROME
}

#[cfg(test)]
mod tests {
    use super::DetailState;

    /// 构造带搜索输入的状态，跑一次搜索，返回 (命中区间, 段偏移)。
    fn run_search(input: &str, data: &[u8], fwd: bool) -> (Option<(usize, usize)>, usize) {
        let mut st = DetailState::default();
        st.val_blob_input = input.to_string();
        let _ = st.blob_search(false, data, fwd);
        (st.blob_hit(false), st.val_seg_off)
    }

    #[test]
    fn blob_search_forward_first() {
        let data = b"xxabcxxabcxx";
        let (hit, off) = run_search("abc", data, true);
        assert_eq!(hit, Some((2, 3)));
        assert_eq!(off, 0);
    }

    #[test]
    fn blob_search_forward_skip_self_and_wrap() {
        let data = b"xxabcxxabcxx";
        let mut st = DetailState::default();
        st.val_blob_input = "abc".into();
        // 第一次：位置 2
        let _ = st.blob_search(false, data, true);
        assert_eq!(st.blob_hit(false), Some((2, 3)));
        // 第二次：位置 7
        let _ = st.blob_search(false, data, true);
        assert_eq!(st.blob_hit(false), Some((7, 3)));
        // 第三次：回绕到 2
        let (_, msg) = st.blob_search(false, data, true);
        assert_eq!(st.blob_hit(false), Some((2, 3)));
        assert!(msg.contains("0x2"));
    }

    #[test]
    fn blob_search_reports_match_count() {
        // VSCode 风格 n/m：命中后记录（当前序号, 总数）；未命中/空输入时清除
        let mut st = DetailState::default();
        let data = b"ab--ab--ab";
        st.val_blob_input = "ab".into();
        st.blob_search(false, data, true);
        assert_eq!(st.blob_matches(false), Some((1, 3)));
        st.blob_search(false, data, true);
        assert_eq!(st.blob_matches(false), Some((2, 3)));
        st.blob_search(false, data, true);
        st.blob_search(false, data, true); // 回绕到第 1 个
        assert_eq!(st.blob_matches(false), Some((1, 3)));
        st.blob_search(false, data, false); // 向前回绕到最后一个
        assert_eq!(st.blob_matches(false), Some((3, 3)));
        // 不重叠计数：aaaa 中搜 aa 应为 2 个
        st.val_blob_input = "aa".into();
        st.blob_search(false, b"aaaa", true);
        assert_eq!(st.blob_matches(false), Some((1, 2)));
        // 未命中清除计数
        st.val_blob_input = "zz".into();
        st.blob_search(false, data, true);
        assert_eq!(st.blob_matches(false), None);
        // 空输入清除计数
        st.val_blob_input = String::new();
        st.blob_search(false, data, true);
        assert_eq!(st.blob_matches(false), None);
    }

    #[test]
    fn blob_search_backward() {
        let data = b"xxabcxxabcxx";
        let mut st = DetailState::default();
        st.val_blob_input = "abc".into();
        // 无命中锚点：全文反向找最后一个
        let _ = st.blob_search(false, data, false);
        assert_eq!(st.blob_hit(false), Some((7, 3)));
        // 继续向前：位置 2
        let _ = st.blob_search(false, data, false);
        assert_eq!(st.blob_hit(false), Some((2, 3)));
        // 再向前：回绕到 7
        let _ = st.blob_search(false, data, false);
        assert_eq!(st.blob_hit(false), Some((7, 3)));
    }

    #[test]
    fn blob_search_hex_input() {
        let data = [0x00, 0x11, 0xDE, 0xAD, 0xBE, 0xEF, 0x22];
        let (hit, _) = run_search("hex(deadbeef)", &data, true);
        assert_eq!(hit, Some((2, 4)));
    }

    #[test]
    fn blob_search_no_match() {
        let data = b"hello world";
        let (hit, _) = run_search("zzz", data, true);
        assert_eq!(hit, None);
    }

    #[test]
    fn blob_search_segments_aligned() {
        // 命中落在第 2 个 64KiB 段：段偏移对齐到 PAGE_BYTES
        let mut data = vec![0u8; crate::fmt::PAGE_BYTES + 100];
        let p = crate::fmt::PAGE_BYTES + 10;
        data[p..p + 3].copy_from_slice(b"zzz");
        let (hit, off) = run_search("zzz", &data, true);
        assert_eq!(hit, Some((p, 3)));
        assert_eq!(off, crate::fmt::PAGE_BYTES);
    }

    #[test]
    fn blob_search_empty_prompt() {
        let mut st = DetailState::default();
        let (_, msg) = st.blob_search(false, b"abc", true);
        assert!(!msg.is_empty());
        assert_eq!(st.blob_hit(false), None);
    }
}
