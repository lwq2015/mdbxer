// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 右侧详情：Key / Value 卡片（格式下拉、复制、文本、hex dump、多值翻页），
//! 以及右栏全部状态（[`DetailState`]）。

use super::syntax::{
    FoldLine, append_colored, append_line_colored, fold_lines, json_palette, syntax_mode,
    syntax_tokens,
};
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
    /// hex dump 紧凑格式：HEX 列字节间不加空格
    pub hex_compact: bool,
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
    /// Key 卡片折叠视图状态（显示文本内容指纹 + 已折叠行号集合；指纹不符自动重置）
    pub key_fold: Option<(u64, Vec<usize>)>,
    /// Value 卡片折叠视图状态
    pub val_fold: Option<(u64, Vec<usize>)>,
    /// Key 卡片折叠视图选区（显示文本指纹 + 字节端点，顺序无关；指纹不符自动重置）
    pub key_fold_sel: Option<(u64, Option<(usize, usize)>)>,
    /// Value 卡片折叠视图选区
    pub val_fold_sel: Option<(u64, Option<(usize, usize)>)>,
    /// Key 卡片 hex 视图选区的内容指纹（段偏移/长度/行/多值序号），用于换内容时清选区
    pub hex_key_fp: Option<(usize, usize, Option<usize>, usize)>,
    /// Key 卡片 hex 视图选区（字节全局序号端点，顺序无关）
    pub hex_key_sel: Option<(usize, usize)>,
    /// Value 卡片 hex 视图选区内容指纹
    pub hex_val_fp: Option<(usize, usize, Option<usize>, usize)>,
    /// Value 卡片 hex 视图选区
    pub hex_val_sel: Option<(usize, usize)>,
    /// Protobuf 载入 .proto 失败的错误信息（会话态，成功载入后清空）
    pub proto_err: Option<String>,
}

impl Default for DetailState {
    fn default() -> Self {
        Self {
            key_mode: DecodeMode::Auto,
            val_mode: DecodeMode::Auto,

            show_addr: true,
            show_hex: true,
            show_ascii: true,
            hex_compact: false,
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

            key_fold: None,
            val_fold: None,

            key_fold_sel: None,
            val_fold_sel: None,

            hex_key_fp: None,
            hex_key_sel: None,
            hex_val_fp: None,
            hex_val_sel: None,

            proto_err: None,
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

    /// 折叠视图状态（只读）：显示文本内容指纹 + 已折叠行号集合。
    pub fn fold_state(&self, is_key: bool) -> &Option<(u64, Vec<usize>)> {
        if is_key {
            &self.key_fold
        } else {
            &self.val_fold
        }
    }

    /// 折叠视图状态（可变）：折叠点击/命中自动展开后写回。
    pub fn fold_state_mut(&mut self, is_key: bool) -> &mut Option<(u64, Vec<usize>)> {
        if is_key {
            &mut self.key_fold
        } else {
            &mut self.val_fold
        }
    }

    /// 折叠视图选区状态（只读）：显示文本内容指纹 + 选区字节端点（顺序无关）。
    pub fn fold_sel(&self, is_key: bool) -> &Option<(u64, Option<(usize, usize)>)> {
        if is_key {
            &self.key_fold_sel
        } else {
            &self.val_fold_sel
        }
    }

    /// 折叠视图选区状态（可变）：拖拽更新/清除后写回。
    pub fn fold_sel_mut(&mut self, is_key: bool) -> &mut Option<(u64, Option<(usize, usize)>)> {
        if is_key {
            &mut self.key_fold_sel
        } else {
            &mut self.val_fold_sel
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
            match bytes[scan..]
                .windows(needle.len())
                .position(|w| w == needle)
            {
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
    // 地址位数随数据大小变化：取 Key/Value 两卡片中较大的最大绝对偏移
    let val_len = app.current_value().map_or(0, |v| v.len());
    let key_len = app
        .selected_row
        .and_then(|i| app.rows.get(i))
        .map_or(0, |r| r.key.len());
    let addr_max_off = (app.detail.key_seg_off + key_len.saturating_sub(1))
        .max(app.detail.val_seg_off + val_len.saturating_sub(1));
    let max_w = detail_max_width(ui, &app.detail, left_w, addr_max_off);
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
                    | ui.add_enabled(
                        app.detail.show_ascii,
                        egui::Checkbox::new(&mut app.detail.show_hex, "HEX"),
                    )
                    .on_hover_text(t.hex_col_tip)
                    .changed()
                    | ui.add_enabled(
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
            let k_text = fmt::decode_key(
                k_win,
                app.detail.key_mode,
                app.endian,
                k_win.len() * 9 + 64,
                &app.hint_key,
            );
            let (_, v_win) = card_window(app, &value, false);
            let v_text = fmt::decode(
                v_win,
                app.detail.val_mode,
                app.endian,
                v_win.len() * 9 + 64,
                &app.hint_value,
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
        let limit =
            (ui.ctx().viewport_rect().width() - left_w - super::MIDDLE_MIN_WIDTH).max(240.0);
        let target = detail_needed_width(ui, &app.detail, addr_max_off).clamp(240.0, limit);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(target, 0.0));
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
    // 行高与 Hex 视图一致（不加 extra_text_line_spacing）：
    // 保证文本区预估高度与实际渲染（fold_view_body / TextEdit）完全吻合。
    let line_h = ui.fonts_mut(|f| f.row_height(&egui::TextStyle::Monospace.resolve(ui.style())));
    let fixed = card_fixed_overhead(total, is_key, dup_nav);
    let (text_open, hex_open) = card_section_open(ui.ctx(), is_key);
    let (text_h, hex_h) = card_content_heights(
        ui,
        app,
        window,
        text,
        line_h,
        f32::INFINITY,
        text_open,
        hex_open,
    );
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
    // 文本自然高度按真实折行计算（与 TextEdit 渲染一致，防止长逻辑行被估成 1 行）；
    // galley 由 egui 全局缓存，重复估算开销可忽略。
    // 16 行封顶只用于"无限预算的自然高度估算"，避免大文本把卡片估成无限高；
    // 有限预算（实际渲染）用真实高度参与下方分配，这样 hex 折叠、预算释放时，
    // 文本区才能和 hex 区一样占满整块空间（否则会永久停在 16 行、下方留白）。
    let text_nat = if text_open {
        let font_id = egui::TextStyle::Monospace.resolve(ui.style());
        let wrap_w = (ui.available_width() - 40.0).max(80.0); // 扣折叠缩进/滚动条/文本框内边距
        let galley =
            ui.fonts_mut(|f| f.layout(text.to_string(), font_id, egui::Color32::WHITE, wrap_w));
        let raw_h = galley.size().y.max(line_h);
        let h = if content_budget.is_finite() {
            raw_h
        } else {
            raw_h.min(16.0 * line_h)
        };
        h + 12.0
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
        // 文本是主阅读区：预算充足时把剩余全部给文本区，使 text_h 足够大，
        // 确保内容能完整显示、不触发滚动条（下方留白由内容本身决定）。
        // auto_shrink 保持收缩：内容不足 text_h 时区域收缩到内容高，不留空白滚动区。
        if text_open {
            return (content_budget - hex_nat, hex_nat);
        }
        return (text_nat, content_budget - text_nat);
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

/// 显示文本内容指纹：折叠状态随文本变化自动失效用（换值/换段/换模式）。
fn text_fp(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// 主题色 → 指定 alpha 的半透明版（折叠视图选区背景，与 hex 视图同法同色）。
fn with_alpha(c: egui::Color32, a: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// 第 `n_chars` 个字符的字节偏移（`n_chars` = 行首起字符数，等于行字符数时为行尾）。
fn char_to_byte(s: &str, n_chars: usize) -> usize {
    s.char_indices().nth(n_chars).map_or(s.len(), |(b, _)| b)
}

/// 指针相对行 galley 左上角的位置 → galley 全文字符偏移（逐视觉行累计，自动处理
/// 折行），再 clamp 到 `line_chars`：折叠摘要后缀不计入选区，落在摘要上视作正文行尾。
fn galley_char_offset(galley: &egui::Galley, p: egui::Pos2, line_chars: usize) -> usize {
    if galley.rows.is_empty() {
        return 0;
    }
    let idx = galley
        .rows
        .iter()
        .position(|r| p.y >= r.pos.y && p.y < r.pos.y + r.row.size.y)
        .unwrap_or(galley.rows.len() - 1);
    let mut off: usize = galley.rows[..idx]
        .iter()
        .map(|r| r.char_count_including_newline().0)
        .sum();
    let row = &galley.rows[idx];
    off += row.row.char_at(p.x - row.pos.x).0;
    off.min(line_chars)
}

/// 折叠视图拖拽选区的过程状态（ctx temp，跨帧保存在卡片组件 id 下）。
#[derive(Clone, Copy)]
struct FoldDrag {
    /// 起手锚点（显示文本字节偏移）
    anchor: usize,
    /// 指针是否真的移出过锚点——区分"单击"（清选区）与"拖选"
    moved: bool,
}

/// 折叠视图：替代 TextEdit 的自绘只读文本渲染（TextEdit 无法隐藏部分行）。
/// 逐行按语法 token 着色（复用 [`crate::ui::syntax`] 的分词与色盘，命中背景
/// 与 TextEdit 路径同色同语义），行首 gutter 画折叠三角（▼ 展开 / ▶ 折叠）；
/// 折叠后隐藏子行，行尾以弱色显示"… N 行"摘要。命中行被折叠遮盖时自动展开
/// 覆盖它的祖先折叠并滚动居中。返回渲染后的折叠状态（内容指纹 + 折叠行号集合），
/// 由调用方写回 [`DetailState`]；祖先自动展开随写回持久生效（与 VS Code 一致）。
///
/// 正文支持拖拽选择与复制（与 hex 视图同交互）：按下定锚点、拖动更新选区端点
/// （可跨行），右键复制所选并取消选择，Ctrl+C 复制选区文本，单击正文/Esc
/// 清除选区；折叠摘要后缀不参与选择。选区随 `sel` 传入并由本函数直接更新，
/// 调用方写回并负责内容指纹失效（换值/换段/换模式时清空）。
#[allow(clippy::too_many_arguments)]
fn fold_view_body(
    ui: &mut egui::Ui,
    is_key: bool,
    text: &str,
    lines: &[FoldLine],
    mode: DecodeMode,
    palette: &[egui::Color32; 6],
    hit: Option<(usize, usize)>,
    scroll_line: Option<usize>,
    stored: &Option<(u64, Vec<usize>)>,
    sel: &mut Option<(usize, usize)>,
) -> (u64, Vec<usize>) {
    let t = tr();
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    // 行高与 Hex 视图一致（不额外加 extra_text_line_spacing），使同窗口内
    // 文本区和 hex 区行高对齐、总高度吻合，避免右栏下方多出/不足。
    let line_h = ui.fonts_mut(|f| f.row_height(&font_id));
    let base = egui::text::TextFormat {
        font_id,
        color: ui.visuals().text_color(),
        line_height: Some(line_h),
        ..Default::default()
    };
    let weak = egui::text::TextFormat {
        color: ui.visuals().weak_text_color(),
        ..base.clone()
    };
    let toks = syntax_tokens(mode, text);
    let fp = text_fp(text);

    let sel_id = egui::Id::new(("mdbxer_fold_sel", is_key));
    let owner_id = egui::Id::new("mdbxer_sel_owner");
    let drag = ui
        .ctx()
        .data(|d| d.get_temp::<Option<FoldDrag>>(sel_id))
        .flatten();
    let sel_bg = with_alpha(ui.visuals().selection.bg_fill, 150);

    let mut folded: std::collections::BTreeSet<usize> = match stored {
        Some((f, set)) if *f == fp => set.iter().copied().collect(),
        _ => std::collections::BTreeSet::new(),
    };
    // 命中行祖先自动展开：所有行号更小且折叠区间覆盖命中行的可折叠行
    if let Some(h) = scroll_line {
        folded.retain(|&i| !(i < h && lines[i].fold_end.is_some_and(|fe| fe >= h)));
    }

    // 行首 gutter：三角点击区固定列宽，正文整体右移对齐
    let gutter = 18.0;
    let wrap_w = (ui.available_width() - gutter - 12.0).max(60.0);
    let mut hide_until = 0usize; // 行号 < hide_until 的行被祖先折叠遮盖，跳过渲染
    let view = ui.clip_rect();
    let content_top = ui.cursor().top();
    let margin = 28.0;
    let dt = ui.input(|i| i.stable_dt).min(0.05);
    for (i, fl) in lines.iter().enumerate() {
        if i < hide_until {
            continue;
        }
        let is_folded = folded.contains(&i);
        let line_str = &text[fl.start..fl.end];
        let line_chars = line_str.chars().count();
        let mut job = egui::text::LayoutJob::default();
        append_line_colored(
            &mut job,
            line_str,
            &toks,
            fl.start,
            fl.end,
            base.clone(),
            palette,
            hit,
        );
        if is_folded {
            let hidden = fl.fold_end.unwrap_or(i) - i;
            job.append(&format!("  {}", t.fold_summary(hidden)), 0.0, weak.clone());
        }
        job.wrap.max_width = wrap_w;
        // 保留行尾空白：保证 glyphs 数与字符数一致，选区字符偏移累计才不错位
        job.keep_trailing_whitespace = true;
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let row_h = galley.size().y.max(line_h);
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), row_h),
            egui::Sense::click_and_drag(),
        );

        // ── 正文拖拽选区：drag 事件只发给起手行，跨行端点更新用全局指针状态 ──
        let in_body = |p: egui::Pos2| p.x >= rect.left() + gutter;
        let press_pos = resp.interact_pointer_pos();
        if resp.drag_started() && press_pos.is_some_and(in_body) {
            let lp = press_pos.unwrap() - egui::vec2(rect.left() + gutter, rect.top());
            let anchor =
                fl.start + char_to_byte(line_str, galley_char_offset(&galley, lp, line_chars));
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    sel_id,
                    Some(FoldDrag {
                        anchor,
                        moved: false,
                    }),
                );
                d.insert_temp(owner_id, Some(sel_id));
            });
        }
        if let Some(dg) = drag {
            if ui.input(|i| i.pointer.primary_down()) {
                if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                    if rect.top() <= p.y && p.y < rect.bottom() && in_body(p) {
                        let lp = p - egui::vec2(rect.left() + gutter, rect.top());
                        let cur = fl.start
                            + char_to_byte(line_str, galley_char_offset(&galley, lp, line_chars));
                        let moved = dg.moved || cur != dg.anchor;
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(
                                sel_id,
                                Some(FoldDrag {
                                    anchor: dg.anchor,
                                    moved,
                                }),
                            )
                        });
                        if moved {
                            *sel = Some((dg.anchor, cur));
                        }
                    }
                }
            }
        }
        if resp.drag_stopped() {
            // 单击正文（没拖动）不保留选区，等同放置光标
            if let Some(FoldDrag { moved: false, .. }) = ui
                .ctx()
                .data(|d| d.get_temp::<Option<FoldDrag>>(sel_id))
                .flatten()
            {
                *sel = None;
            }
            ui.ctx()
                .data_mut(|d| d.remove_temp::<Option<FoldDrag>>(sel_id));
        }
        // 右键单击正文：有选区时复制并取消选择（右键 = 复制 + 取消所选）；
        // 单击正文/Esc 仍取消选择
        if resp.secondary_clicked() && press_pos.is_some_and(in_body) {
            if let Some((a, b)) = sel.take() {
                let (lo, hi) = (a.min(b), a.max(b));
                ui.ctx().copy_text(text[lo..hi].to_owned());
            }
        }
        // 正文悬停显示文本光标（gutter/三角区域保持各自光标）
        if resp.hovered() && ui.input(|i| i.pointer.hover_pos()).is_none_or(in_body) {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::Text);
        }

        // 选区背景：正文选中字符逐 glyph 画半透明底色（先于文字绘制；
        // 摘要后缀不在选区内，天然不着色；折行后跨视觉行也按字符精确覆盖）
        if let Some((a, b)) = *sel {
            let lo = a.min(b).max(fl.start);
            let hi = a.max(b).min(fl.end);
            if lo < hi {
                let cs = line_str[..lo - fl.start].chars().count();
                let ce = cs + line_str[lo - fl.start..hi - fl.start].chars().count();
                let origin = rect.left_top() + egui::vec2(gutter, 0.0);
                let mut char_off = 0usize;
                'glyphs: for prow in &galley.rows {
                    for (gi, g) in prow.row.glyphs.iter().enumerate() {
                        let idx = char_off + gi;
                        if idx >= ce {
                            break 'glyphs;
                        }
                        if idx >= cs {
                            let grect = egui::Rect::from_min_size(
                                origin + prow.pos.to_vec2() + egui::vec2(g.pos.x, 0.0),
                                egui::vec2(g.advance_width, prow.row.size.y),
                            );
                            ui.painter().rect_filled(grect, 1.5, sel_bg);
                        }
                    }
                    char_off += prow.char_count_including_newline().0;
                }
            }
        }

        ui.painter().galley(
            rect.left_top() + egui::vec2(gutter, 0.0),
            galley,
            base.color,
        );

        if fl.fold_end.is_some() {
            // 折叠三角：行首 gutter 内、首行垂直居中；▼=展开 ▶=已折叠
            let c = ui.visuals().weak_text_color();
            let x = rect.left() + 4.0;
            let y = rect.top() + (line_h - 8.0) * 0.5;
            let (pts, tri_rect) = if is_folded {
                (
                    vec![
                        egui::pos2(x + 2.0, y),
                        egui::pos2(x + 8.0, y + 4.0),
                        egui::pos2(x + 2.0, y + 8.0),
                    ],
                    egui::Rect::from_min_size(egui::pos2(x, y - 2.0), egui::vec2(14.0, 12.0)),
                )
            } else {
                (
                    vec![
                        egui::pos2(x, y + 2.0),
                        egui::pos2(x + 8.0, y + 2.0),
                        egui::pos2(x + 4.0, y + 8.0),
                    ],
                    egui::Rect::from_min_size(egui::pos2(x - 2.0, y), egui::vec2(12.0, 14.0)),
                )
            };
            let resp = ui.interact(
                tri_rect,
                egui::Id::new(("fold_tri", is_key, i)),
                egui::Sense::click(),
            );
            if resp.clicked() {
                if is_folded {
                    folded.remove(&i);
                } else {
                    folded.insert(i);
                }
            }
            resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            ui.painter()
                .add(egui::Shape::convex_polygon(pts, c, egui::Stroke::NONE));
        }

        if scroll_line == Some(i) {
            ui.scroll_to_rect(rect, Some(egui::Align::Center));
        }
        if is_folded {
            hide_until = fl.fold_end.unwrap_or(i) + 1;
        }
    }

    // ── 拖到视口上下边缘自动滚动：使选区可以一直延伸到当前不可见的部分
    //    （与 hex 视图同交互）。速度按指针深入边缘的距离比例递增（二次方——
    //    贴边缓慢精修，越出边缘越远越快；上限约 60 行高/秒）；内容本身没超出
    //    该方向则不滚。滚动帧端点不更新，下一帧内容滚到位后由行命中逻辑延伸。──
    if drag.is_some() && ui.input(|i| i.pointer.primary_down()) {
        if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
            let max_speed = line_h * 60.0; // 点/秒
            if content_top < view.top() - 1.0 && p.y < view.top() + margin {
                let k = ((view.top() + margin - p.y) / margin).clamp(0.0, 4.0);
                // egui 约定 scroll delta 正值 = 向上看
                ui.scroll_with_delta(egui::vec2(0.0, max_speed * k * k * dt));
            } else if ui.cursor().top() > view.bottom() + 1.0 && p.y > view.bottom() - margin {
                let k = ((p.y - (view.bottom() - margin)) / margin).clamp(0.0, 4.0);
                ui.scroll_with_delta(egui::vec2(0.0, -max_speed * k * k * dt));
            }
        }
    }

    // ── 键盘：本视图为最近选择交互目标时，Ctrl+C 复制选区文本、Esc 清除 ──
    // 多行多 widget 无法共享一个焦点 id，改用"最近按下"的视图标记（与 hex
    // 视图共用 owner id）判断快捷键归属，避免两个视图同时响应互相覆盖剪贴板。
    let owner = ui
        .ctx()
        .data(|d| d.get_temp::<Option<egui::Id>>(owner_id))
        .flatten();
    if owner == Some(sel_id) {
        let (copy, escape) = ui.input(|i| {
            (
                i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Copy | egui::Event::Cut)),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if copy {
            if let Some((a, b)) = *sel {
                let (lo, hi) = (a.min(b), a.max(b));
                ui.ctx().copy_text(text[lo..hi].to_owned());
            }
        }
        if escape {
            *sel = None;
        }
    }
    (fp, folded.into_iter().collect())
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
    let save_base = if is_key {
        "key".to_string()
    } else if dup_sort {
        format!("value_{:06}", app.detail.dup_index + 1)
    } else {
        "value".to_string()
    };
    egui::Frame::group(ui.style())
        .show(ui, |ui| {
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
                        // Auto 下压缩容器：解压后内层为结构化 JSON/BSON/MSGPACK
                        // 时按内层格式复制 pretty，与详情显示一致。
                        let decomp_copy = (app.detail.mode_of(is_key) == DecodeMode::Auto)
                            .then(|| fmt::decompress(bytes))
                            .flatten()
                            .and_then(|(_, inner)| {
                                syntax_mode(DecodeMode::Auto, &inner).map(|m| (m, inner))
                            });

                        if let Some((m, inner)) = decomp_copy {
                            ui.ctx().copy_text(fmt::decode(
                                &inner,
                                m,
                                app.endian,
                                usize::MAX,
                                &app.hint_value,
                            ));
                        } else if !is_key
                            && let Some(pretty) =
                                proto_auto_pretty(app.detail.mode_of(is_key), bytes)
                        {
                            // Auto + 已载入协议池：试探出 message 类型时按该类型
                            // 复制 proto3 JSON pretty，与详情显示一致
                            ui.ctx().copy_text(pretty);
                        } else {
                            // Auto 下完整值是结构化格式（BSON/JSON/MSGPACK/CBOR/XML）时
                            // 按对应格式复制 pretty，与详情显示一致；其余按当前实际模式复制。
                            // Key 不做特殊格式猜测（与 Auto 猜型策略一致），Auto 直接按原文复制。
                            let copy_mode = match app.detail.mode_of(is_key) {
                                DecodeMode::Auto if !is_key => {
                                    syntax_mode(DecodeMode::Auto, bytes).unwrap_or(DecodeMode::Auto)
                                }
                                m => m,
                            };
                            let hints = if is_key {
                                &app.hint_key
                            } else {
                                &app.hint_value
                            };
                            ui.ctx()
                                .copy_text(text_of(bytes, copy_mode, app.endian, hints, is_key));
                        }
                    }
                    // 另存菜单：原始二进制 / 可见文本 / 猜测格式（Auto 且命中时）
                    let _save_resp = ui
                        .menu_button(t.save_as, |ui| {
                            if ui.button(t.save_raw).clicked() {
                                ui.close();
                                app.save_bytes(&format!("{save_base}.bin"), bytes, "bin");
                            }
                            let hints = if is_key {
                                &app.hint_key
                            } else {
                                &app.hint_value
                            };
                            if ui.button(t.save_text).clicked() {
                                ui.close();
                                let text = text_of(
                                    bytes,
                                    app.detail.mode_of(is_key),
                                    app.endian,
                                    hints,
                                    is_key,
                                );
                                app.save_bytes(&format!("{save_base}.txt"), text.as_bytes(), "txt");
                            }
                            // 猜测格式：仅 Value 且 Auto 模式（Key 不做特殊格式猜测，
                            // 与猜型策略一致；手动模式下"可见文本"已是该格式文本）
                            if !is_key
                                && app.detail.mode_of(is_key) == DecodeMode::Auto
                                && let Some((label, data, ext)) = guess_save(bytes)
                            {
                                if ui.button(format!("{label}（.{ext}）")).clicked() {
                                    ui.close();
                                    app.save_bytes(&format!("{save_base}.{ext}"), &data, ext);
                                }
                            }
                        })
                        .response
                        .on_hover_text(t.save_tip);
                    // 格式下拉
                    let mut mode = app.detail.mode_of(is_key);
                    let flat = DecodeMode::flat_all();
                    let ir = egui::ComboBox::from_id_salt(("detail_mode", is_key))
                        .selected_text(mode.label())
                        .height(430.0)
                        .show_ui(ui, |ui| {
                            // 自动项置顶；其余按"文本编码/数值/字节视图/标识与时间/结构化"分组
                            ui.selectable_value(
                                &mut mode,
                                DecodeMode::Auto,
                                DecodeMode::Auto.label(),
                            );
                            for (grp, items) in super::format_groups() {
                                // 组标题：弱化小字，不可选中
                                ui.add(egui::Label::new(egui::RichText::new(grp).weak().small()));
                                for m in items {
                                    ui.selectable_value(&mut mode, *m, m.label());
                                }
                            }
                        });
                    super::wheel_cycle(ui.ctx(), &ir.response, &flat, &mut mode);
                    // 详情卡片排版独立于顶栏表格排版：只改本卡片，不回写全局、不存偏好。
                    if app.detail.mode_of(is_key) != mode {
                        app.detail.set_mode(is_key, mode);
                    }
                });
            });

            // Protobuf：message 类型选择 + .proto 载入。类型池与当前选择是全局的
            // （Key/Value 两卡共用、重启沿用），仅在选择 proto 排版时显示本行。
            if app.detail.mode_of(is_key) == DecodeMode::Proto {
                ui.horizontal(|ui| {
                    let names = crate::proto::message_names();
                    let mut cur = crate::proto::current();
                    let sel_text = cur.clone().unwrap_or_else(|| t.proto_pick.to_string());

                    // message 类型下拉（全名 pkg.Msg；空池显示占位项）
                    let ir = egui::ComboBox::from_id_salt(("proto_type", is_key))
                        .selected_text(sel_text)
                        .width(180.0)
                        .height(430.0)
                        .show_ui(ui, |ui| {
                            if names.is_empty() {
                                ui.add_enabled(false, egui::Label::new(t.proto_no_types));
                            }
                            for n in &names {
                                if ui.selectable_value(&mut cur, Some(n.clone()), n).changed() {
                                    crate::proto::set_current(cur.clone());
                                    save_proto_binding(app, cur.as_deref());
                                }
                            }
                        });
                    ir.response.on_hover_text(t.proto_pick);

                    // 载入 .proto（可多选；失败保留旧池并显示错误）
                    if ui
                        .small_button(t.proto_load)
                        .on_hover_text(t.proto_load_tip)
                        .clicked()
                    {
                        if let Some(paths) = rfd::FileDialog::new()
                            .add_filter("Protobuf", &["proto"])
                            .pick_files()
                        {
                            match crate::proto::load_files(&paths) {
                                Ok(_) => {
                                    app.detail.proto_err = None;
                                    save_proto_binding(app, crate::proto::current().as_deref());
                                }
                                Err(e) => app.detail.proto_err = Some(e),
                            }
                        }
                    }

                    // 清除关联：删除本库的 .proto 绑定并清空载入状态
                    if ui
                        .small_button(t.proto_clear)
                        .on_hover_text(t.proto_clear_tip)
                        .clicked()
                    {
                        if let Some(p) = &app.opened_path {
                            crate::config::clear_proto_binding(p);
                        }
                        crate::proto::clear();
                        app.detail.proto_err = None;
                    }

                    // 载入失败提示：截断显示，hover 看全文
                    if let Some(e) = &app.detail.proto_err {
                        let short: String = e.chars().take(60).collect();
                        ui.label(egui::RichText::new(format!("⚠ {short}")).small().weak())
                            .on_hover_text(e);
                    }
                });
            }

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
                    let key_enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let (ctrl, shift) = ui.input(|i| (i.modifiers.ctrl, i.modifiers.shift));
                    // 无修饰 Enter 会 surrender focus（lost_focus）；带修饰 Enter 焦点仍在（has_focus）。
                    let enter = key_enter && (resp.lost_focus() || resp.has_focus());
                    // Esc：清除搜索输入。egui 0.36 无修饰 Esc 帧首收走焦点，用 lost_focus 判据。
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        input.clear();
                    }
                    // Enter=下一个；Shift+Enter 或 Ctrl+Enter=上一个
                    // ⬇(下一个)在左、⬆(上一个)在右：主操作（Enter）优先的习惯顺序
                    let next = ui.button("⬇").on_hover_text(t.search_next_tip).clicked()
                        || (enter && !shift && !ctrl);
                    let prev = ui.button("⬆").on_hover_text(t.search_prev_tip).clicked()
                        || (enter && (shift || ctrl));
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
                        let key_enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let (ctrl, shift) =
                            ui.input(|i| (i.modifiers.ctrl, i.modifiers.shift));
                        // 无修饰 Enter 会 surrender focus（lost_focus）；带修饰 Enter 焦点仍在（has_focus）。
                        let enter = key_enter && (resp.lost_focus() || resp.has_focus());
                        // Esc：清除搜索输入（egui 无修饰 Esc 帧首收走焦点，用 lost_focus 判据）。
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            app.detail.dup_search_input.clear();
                        }
                        // Enter=下一个；Shift+Enter 或 Ctrl+Enter=上一个
                        // ⬇(下一个)在左、⬆(上一个)在右，与大字段搜索行一致
                        let next = ui.button("⬇").on_hover_text(t.search_next_tip).clicked()
                            || (enter && !shift && !ctrl);
                        let prev = ui.button("⬆").on_hover_text(t.search_prev_tip).clicked()
                            || (enter && (shift || ctrl));
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
            // 行高与 Hex 视图一致（不加 extra_text_line_spacing）：与
            // fold_view_body / TextEdit layouter 的实测行高保持一致。
            let line_h = ui.fonts_mut(|f| f.row_height(&egui::TextStyle::Monospace.resolve(ui.style())));
            let fixed = card_fixed_overhead(total, is_key, is_dup);
            let (text_open, hex_open) = card_section_open(ui.ctx(), is_key);
            // 语法着色：手动 Json/Yaml/Toml/Xml（内容合法）或 Auto 命中
            // 结构化 JSON/BSON/MSGPACK/CBOR/XML 时启用；Some 同时给出 Auto 下
            // pretty 重解码应使用的格式。Key 仅 Auto 不做特殊格式猜测（不着色），
            // 手动 Json/Yaml/Toml/Xml 照常着色。
            let key_auto = is_key && app.detail.mode_of(is_key) == DecodeMode::Auto;
            let json_mode = if key_auto {
                None
            } else {
                syntax_mode(app.detail.mode_of(is_key), window)
            };
            // Auto + 已载入协议池：全池试探出 message 类型时按该类型 pretty 渲染
            //（着色复用 JSON 语法——proto3 JSON mapping 输出即 JSON 文本）；
            // 二者仅在 Auto 生效，Key 的 Auto 不做特殊处理
            let proto_pretty = if key_auto {
                None
            } else {
                proto_auto_pretty(app.detail.mode_of(is_key), window)
            };
            // Auto 下压缩容器穿透：解压后内层仍是结构化数据时按内层格式 pretty/着色
            let decomp = (!key_auto && app.detail.mode_of(is_key) == DecodeMode::Auto)
                .then(|| fmt::decompress(window))
                .flatten()
                .and_then(|(_, inner)| syntax_mode(DecodeMode::Auto, &inner).map(|m| (m, inner)));
            // 着色格式：Auto 命中格式 > 压缩内层格式 > proto（恒 JSON pretty）
            let syntax_kind = json_mode
                .or_else(|| decomp.as_ref().map(|(m, _)| *m))
                .or_else(|| proto_pretty.as_ref().map(|_| DecodeMode::Json));
            // 详情文本区"实际显示"的文本：Auto 命中结构化格式（JSON/BSON/CBOR/
            // MSGPACK/XML）时改用 pretty。guess 返回的是紧凑原文（保证表格单行
            // 预览不被破坏），故这里要重解码一次。
            // 高度估算与下方渲染必须共用这份文本，否则 pretty 后行数变多却仍按紧凑文本
            // 的 1 行估高，文本区会被裁掉大半、也无法占满折叠 hex 释放出的空间。
            let display_text: String = match decomp {
                Some((m, inner)) => fmt::decode(
                    &inner,
                    m,
                    app.endian,
                    inner.len().saturating_mul(4) + 256,
                    &app.hint_value,
                ),
                None => {
                    if let Some(pretty) = proto_pretty {
                        pretty
                    } else {
                        match (json_mode, app.detail.mode_of(is_key)) {
                            (Some(m), DecodeMode::Auto) => fmt::decode(
                                window,
                                m,
                                app.endian,
                                window.len().saturating_mul(4) + 256,
                                &app.hint_value,
                            ),
                            _ => text.to_string(),
                        }
                    }
                }
            };
            let syntax_palette = json_palette(ui.visuals().dark_mode);
            // hex 份额不在这里定：渲染时按游标实测回填（见下），此处只分文本区
            let (text_h, _) = card_content_heights(
                ui,
                app,
                window,
                &display_text,
                line_h,
                (budget - fixed).max(80.0),
                text_open,
                hex_open,
            );
            // 猜测/字节数信息紧跟"文本"标题文字同一行右侧（自定义标题行），
            // 不再单列一行占据文本框与 hex 之间的位置
            let guess_text = if app.detail.mode_of(is_key) == DecodeMode::Auto {
                let (label, _) = fmt::guess(bytes, app.endian, &app.hint_value);
                // 附加解读只保留文件魔数（强信号、短标注）：地址/FILETIME 对
                // 任意字节几乎总命中（IPv4/IPv6 无有效性约束），放猜测行是噪音
                // 且撑宽窗口——需要时切手动 mac/ip/time 格式查看
                match fmt::magic_hint(bytes) {
                    Some(h) => t.guess_line(&format!("{label} · {h}"), bytes.len()),
                    None => t.guess_line(&label, bytes.len()),
                }
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
            // 文本区命中高亮区间（文本字节坐标）：仅当"实际显示文本"与窗口字节 1:1
            // （即显示原文，长度相等且为合法 UTF-8）时可靠；hex/数值/pretty JSON 等
            // 重编码显示不映射，只滚动不高亮——pretty JSON 与手动 Json 模式行为一致。
            let text_hit_range: Option<(usize, usize)> = (|| {
                let (hit_abs, hit_len) = app.detail.blob_hit(is_key)?;
                if hit_abs < off || hit_abs >= off + window.len() {
                    return None;
                }
                if display_text.len() != window.len() || std::str::from_utf8(window).is_err() {
                    return None;
                }
                let mut hs = hit_abs - off;
                let mut he = (hs + hit_len).min(display_text.len());
                while hs > 0 && !display_text.is_char_boundary(hs) {
                    hs -= 1;
                }
                while he < display_text.len() && !display_text.is_char_boundary(he) {
                    he += 1;
                }
                (hs < he).then_some((hs, he))
            })();
            // 折叠视图：语法着色激活且存在可折叠块（pretty 多行结构化文本）时启用，
            // 文本区改为自绘逐行渲染（TextEdit 无法隐藏部分行）；单行紧凑 JSON、
            // 纯文本、无块的多行文本仍走 TextEdit 原路径，零行为变化。
            // pretty JSON 等重编码显示下 text_hit_range 本就为 None（不映射），
            // 1:1 文本（如原样多行 JSON）则命中背景在折叠视图中继续生效。
            let fold_lines_list = match syntax_kind {
                Some(_) if display_text.contains('\n') => fold_lines(&display_text),
                _ => Vec::new(),
            };
            let fold_active = fold_lines_list.iter().any(|f| f.fold_end.is_some());
            // 折叠视图滚动目标行：显示文本与窗口字节 1:1 时按字节精确定位行，
            // 重编码显示（pretty）按字节比例估算；命中行被祖先折叠遮盖时，
            // fold_view_body 渲染中自动展开覆盖它的祖先折叠
            let fold_scroll_line = if fold_active {
                text_scroll_to.and_then(|hit_abs| {
                    (hit_abs >= off && hit_abs < off + window.len()).then(|| {
                        let hit_rel = hit_abs - off;
                        let mut bp = if display_text.len() == window.len()
                            && std::str::from_utf8(window).is_ok()
                        {
                            hit_rel
                        } else {
                            display_text.len() * hit_rel / window.len().max(1)
                        };
                        while bp > 0 && !display_text.is_char_boundary(bp) {
                            bp -= 1;
                        }
                        fold_lines_list
                            .partition_point(|fl| fl.start <= bp)
                            .saturating_sub(1)
                    })
                })
            } else {
                None
            };
            let fold_stored = app.detail.fold_state(is_key).clone();
            // 折叠视图选区：内容指纹不符（换值/换段/换模式）时清空
            let fold_sel_fp = text_fp(&display_text);
            let mut fold_sel_writeback: Option<(usize, usize)> = match app.detail.fold_sel(is_key) {
                Some((f, Some(r))) if *f == fold_sel_fp => Some(*r),
                _ => None,
            };
            let mut fold_writeback: Option<(u64, Vec<usize>)> = None;
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
                if fold_active {
                    // 折叠视图：自绘逐行渲染 + 行首折叠三角（替代 TextEdit）。
                    // 与 TextEdit 路径同一 ScrollArea 约束（id_salt 相同，滚动位置
                    // 在两种视图间延续）；返回渲染后的折叠状态（含点击切换与命中
                    // 自动展开祖先），借 fold_writeback 带出闭包、结束后写回。
                    fold_writeback = Some(
                        egui::ScrollArea::vertical()
                            .id_salt(("detail_text_scroll", is_key))
                            .auto_shrink([false, true])
                            .max_height(text_h)
                            .show(ui, |ui| {
                                let out = fold_view_body(
                                    ui,
                                    is_key,
                                    &display_text,
                                    &fold_lines_list,
                                    syntax_kind.unwrap_or(DecodeMode::Json),
                                    &syntax_palette,
                                    text_hit_range,
                                    fold_scroll_line,
                                    &fold_stored,
                                    &mut fold_sel_writeback,
                                );
                                out
                            })
                            .inner,
                    );
                    return;
                }
                // display_text 已按当前模式算好（Auto 命中结构化 JSON 时为 pretty，
                // 其余为原文）；高度估算也用它，二者一致。
                let mut text_owned = display_text.clone();
                // + 12 ≈ TextEdit frame 上下内边距，保证整数行内容恰好不滚
                egui::ScrollArea::vertical()
                    .id_salt(("detail_text_scroll", is_key))
                    .auto_shrink([false, true])
                    .max_height(text_h)
                            .show(ui, |ui| {
                                let content_top = ui.cursor().min.y;
                                // 记录 layouter 拿到的实际 wrap 宽度（与 TextEdit 渲染完全一致），
                                // 供下方命中滚动定位复用它构图——估算 wrap 会导致大文本折行次数
                                // 与渲染不一致，命中行越靠后偏差累积越大，滚过头滚出视口。
                                // Rc 让 move 闭包与滚动定位共享同一个 Cell。
                                let hit_wrap = std::rc::Rc::new(std::cell::Cell::new(None::<f32>));
                                let hit_wrap_layouter = hit_wrap.clone();
                                let mut te = egui::TextEdit::multiline(&mut text_owned)
                                    .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(1);
                        // 命中区间琥珀色高亮（与 hex 视图同色半透明，深浅主题均可读）；
                        // JSON 模式下额外做语法着色，二者靠 TextFormat 的 color/background 叠加。
                        // 闭包先绑定变量再取 &mut，否则临时量在 add(te) 前就被释放（E0716）
                        // 必须与 egui 默认 multiline layouter 行为一致：wrap 宽度生效折行、
                        // 逐段设行高、保留行尾空白——否则单行长内容不折行，文本区只剩一截
                        // 横向被裁的"一行"，看起来就像详情内容消失
                        let palette = syntax_palette;
                        let mut hl_layouter =
                            move |ui: &egui::Ui, s: &dyn egui::TextBuffer, wrap: f32| {
                                hit_wrap_layouter.set(Some(wrap)); // 记录实际折行宽度供滚动定位复用
                                let s = s.as_str();
                                let font_id = egui::TextStyle::Monospace.resolve(ui.style());
                                // 与 Hex 视图行高一致：不加 extra_text_line_spacing，
                                // 同窗口文本/hex 行高对齐（egui TextEdit 默认也不加）。
                                let line_height = ui.fonts_mut(|f| f.row_height(&font_id));
                                let base = egui::text::TextFormat {
                                    font_id,
                                    color: ui.visuals().text_color(),
                                    line_height: Some(line_height),
                                    ..Default::default()
                                };
                                let mut job = egui::text::LayoutJob::default();
                                if let Some(mode) = syntax_kind {
                                    // s 即整段解码文本 text（合法结构化文本，pretty 多行），
                                    // 按格式分派词法分词器着色。
                                    let toks = syntax_tokens(mode, s);
                                    append_colored(
                                        &mut job,
                                        s,
                                        &toks,
                                        base,
                                        &palette,
                                        text_hit_range,
                                    );
                                } else if let Some((hs, he)) = text_hit_range {
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
                        // 无条件走自定义 layouter：即使无高亮/无语法，也要让 hit_wrap
                        // 记录到 TextEdit 实际折行宽度。否则折行宽度靠估算，与 TextEdit
                        // 渲染的折行不一致，命中滚动定位算出的行数就会偏（长文本尤其）。
                        // 无高亮时该 layouter 行为与 egui 默认 multiline 一致（同 wrap、
                        // 同 row_height、保留尾空白），故可安全无条件使用。
                        te = te.layouter(&mut hl_layouter);
                        let te_rect = ui.add(te).rect;
                        // 搜索命中：滚到命中所在行。这里必须在与渲染/高亮**同一文本
                        // display_text** 上构图（不能再用 text）。折行宽度也尽量贴近
                        // TextEdit 实际内容宽度（frame 左右内边距约各 6pt），否则
                        // blob 长文本折行次数与 TextEdit 不一致，行号累加后定位偏移，
                        // 命中行滚出视口——即"高亮不同步定位、目标不可见"。
                        if let Some(hit_abs) = text_scroll_to {
                            if hit_abs >= off && hit_abs < off + window.len() {
                                let dt = &display_text;
                                // 命中字符偏移（基于显示文本 display_text，与高亮/折行同源）：
                        // 1:1 时直接取高亮起点或字节转字符；非 1:1（lossy 重编码的二进制
                        // blob）按字符比例近似——每行字数比字节数均匀，比字节比例更贴近
                        // 视觉折行，消除命中越靠后字节比例偏差累积导致的滚过头。
                        let pc = if let Some((hs, _)) = text_hit_range {
                            dt[..hs].chars().count()
                        } else if std::str::from_utf8(window).is_ok() {
                            dt.chars().take(hit_abs - off).count()
                        } else {
                            dt.chars().count() * (hit_abs - off) / window.len().max(1)
                        };
                        let font_id = egui::TextStyle::Monospace.resolve(ui.style());
                        // 与 TextEdit 渲染完全一致的 wrap（layouter 实际给的值）；
                        // 仅当未走自定义 layouter（无高亮映射）时才回退估算宽度。
                        let wrap_w = hit_wrap
                            .get()
                            .unwrap_or((te_rect.width() - 12.0).max(40.0));
                        let galley = ui.fonts_mut(|f| {
                            f.layout(dt.to_string(), font_id, egui::Color32::WHITE, wrap_w)
                        });
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
            // 折叠状态写回：渲染中可能因点击折叠三角或命中自动展开祖先而变化；
            // 内容指纹随状态一起存，换值/换段/换模式后指纹不符即整体重置为全展开
            if let Some(wb) = fold_writeback {
                *app.detail.fold_state_mut(is_key) = Some(wb);
                // 折叠视图本帧渲染过：选区状态同步写回（无选区时存 None，
                // 指纹随状态一起存，换值/换段/换模式后指纹不符即清空）
                *app.detail.fold_sel_mut(is_key) = Some((fold_sel_fp, fold_sel_writeback));
            }

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
                            app.detail.hex_compact,
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

/// 复制按钮用：完整解码文本（不受 cell_max 截断）；Key 不做特殊格式猜测。
fn text_of(
    bytes: &[u8],
    mode: DecodeMode,
    endian: fmt::Endian,
    hints: &fmt::HintFlags,
    is_key: bool,
) -> String {
    if is_key {
        fmt::decode_key(bytes, mode, endian, usize::MAX, hints)
    } else {
        fmt::decode(bytes, mode, endian, usize::MAX, hints)
    }
}

/// 保存当前库的 Protobuf 关联表（.proto 文件列表 + message 选择），
/// 打开该库时自动恢复。未打开库（无宿主可挂）时跳过。
fn save_proto_binding(app: &MdbxerApp, msg: Option<&str>) {
    if let Some(p) = &app.opened_path {
        crate::config::save_proto_binding(p, &crate::proto::files(), msg);
    }
}

/// Auto 模式 + 已载入协议池：全池试探解码（unknown 字段判别），命中时
/// 返回按该类型的 proto3 JSON pretty 文本（着色层复用 JSON 语法渲染）。
/// 未载入池/非 Auto/超长/无 0-unknown 候选时返回 None，走原文本管线。
fn proto_auto_pretty(mode: DecodeMode, bytes: &[u8]) -> Option<String> {
    if mode != DecodeMode::Auto
        || !crate::proto::is_loaded()
        || bytes.len() > crate::proto::GUESS_MAX
    {
        return None;
    }

    crate::proto::guess_candidates(bytes)
        .into_iter()
        .find(|(_, unknown)| *unknown == 0)
        .and_then(|(name, _)| crate::proto::decode_with(&name, bytes))
}

/// 魔数格式名 → 推荐文件扩展名。
fn magic_ext(m: &str) -> &'static str {
    match m {
        "JPEG" => "jpg",
        "PNG" => "png",
        "GIF" => "gif",
        "WebP" => "webp",
        "WAV" => "wav",
        "MP3" => "mp3",
        "Ogg" => "ogg",
        "FLAC" => "flac",
        "ZIP" => "zip",
        "7z" => "7z",
        "RAR" => "rar",
        "PDF" => "pdf",
        "MP4" => "mp4",
        "ICO" => "ico",
        "SQLite" => "db",
        _ => "bin",
    }
}

/// 结构化解码模式 → (扩展名, 菜单标签)；JSON 类（含 BSON/MSGPACK/CBOR）
/// 统一落 JSON。
fn structured_ext(mode: DecodeMode) -> (&'static str, &'static str) {
    match mode {
        DecodeMode::Xml => ("xml", "XML"),
        DecodeMode::Yaml => ("yaml", "YAML"),
        DecodeMode::Toml => ("toml", "TOML"),
        _ => ("json", "JSON"),
    }
}

/// 内容嗅探：推荐文件扩展名（结构化 → 魔数 → 兜底 bin）。
fn sniff_ext(bytes: &[u8]) -> &'static str {
    if let Some(mode) = syntax_mode(DecodeMode::Auto, bytes) {
        return structured_ext(mode).0;
    }
    if let Some(m) = fmt::magic_hint(bytes) {
        return magic_ext(m);
    }
    "bin"
}

/// 另存菜单"猜测格式"候选：与 Auto 显示管线同优先级（压缩穿透 →
/// 结构化 pretty → PROTO → 二进制魔数），返回 (标签, 内容, 推荐扩展名)；
/// 无命中返回 None（菜单不出现该项）。
fn guess_save(bytes: &[u8]) -> Option<(String, Vec<u8>, &'static str)> {
    // 压缩容器：存解压后的内层，标签穿透内层猜测（如 "gzip → JSON"）
    if let Some((algo, inner)) = fmt::decompress(bytes) {
        let (label, _) = fmt::guess(&inner, fmt::Endian::Little, &fmt::HintFlags::default());
        let ext = sniff_ext(&inner);
        return Some((format!("{algo} → {label}"), inner, ext));
    }

    // 结构化格式：存 pretty 文本（BSON/MSGPACK/CBOR 统一转 JSON）
    if let Some(mode) = syntax_mode(DecodeMode::Auto, bytes) {
        let (ext, label) = structured_ext(mode);
        let text = fmt::decode(
            bytes,
            mode,
            fmt::Endian::Little,
            usize::MAX,
            &fmt::HintFlags::default(),
        );
        return Some((format!("{label} pretty"), text.into_bytes(), ext));
    }

    // PROTO：存 proto3 JSON 文本
    if let Some(text) = proto_auto_pretty(DecodeMode::Auto, bytes) {
        return Some(("PROTO · JSON".to_string(), text.into_bytes(), "json"));
    }

    // 二进制魔数：原始字节配正确扩展名（JPEG/ZIP 等本体即该格式）
    if let Some(m) = fmt::magic_hint(bytes) {
        let ext = magic_ext(m);
        return Some((m.to_string(), bytes.to_vec(), ext));
    }

    None
}

/// 右栏宽度上限：保证当前 hex 配置下最长一行（16 字节时最宽）
/// 在面板内不折行。按等宽字体实测字宽计算，再扣除各级边距；
/// 同时不超过窗口宽度减去左栏与中央表格最小保留宽度，
/// 窄窗口时下限放宽到右栏最小宽度 240，优先保证中央表格。
fn detail_max_width(ui: &egui::Ui, d: &DetailState, left_w: f32, addr_max_off: usize) -> f32 {
    let needed = detail_needed_width(ui, d, addr_max_off);
    let screen = ui.ctx().viewport_rect().width();
    needed
        .max(720.0)
        .min((screen - left_w - super::MIDDLE_MIN_WIDTH).max(240.0))
}

/// 当前 hex 配置（行宽 + 地址/HEX/ASCII 开关）下内容实际所需宽度。
/// `addr_max_off` 为当前数据（Key/Value 中较大者）的最大绝对偏移，决定地址列位数。
fn detail_needed_width(ui: &egui::Ui, d: &DetailState, addr_max_off: usize) -> f32 {
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font_id, '0'));

    let n = d.hex_width as f32;
    let mut line_chars = 0.0_f32;
    if d.show_addr {
        // 地址位数随数据大小变化（最小 4 位）+ 列后 2 空格
        line_chars += crate::fmt::addr_chars(crate::fmt::addr_digits(0, addr_max_off + 1)) as f32;
    }
    if d.show_hex {
        // 每字节 2~3 字符（紧凑 2 / 常规 3），宽行中间额外 1 个分隔空格
        line_chars += crate::fmt::hex_section_chars(d.hex_width, d.hex_compact) as f32;
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
