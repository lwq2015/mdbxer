//! 界面层：应用状态与导航动作。只接触 db 层暴露的纯数据结构。

mod dataview;
mod detail;
mod sidebar;
mod statsview;
mod topbar;

use crate::db::{self, Anchor, DbHandle, Direction, JumpKey, OpenMode, Row, TableInfo};
use crate::fmt::DecodeMode;
use crate::history::History;

pub const PAGE_SIZES: [usize; 5] = [50, 100, 200, 500, 1000];
pub const DEFAULT_PAGE_SIZE: usize = 200;
pub const DEFAULT_CELL_MAX: usize = 256;
pub const DUP_PAGE_SIZE: usize = 100;

/// 让 ComboBox 支持滚轮：悬停在按钮上（未展开）时逐格切换选项。
/// - 鼠标滚轮：一个刻度事件切换一项（不跳格）
/// - 触控板像素滚动：累计满 40 点切换一项
/// - 到达首尾时停止，不循环
/// 返回 true 表示选项被滚轮改变。
pub fn wheel_cycle<T: Copy + PartialEq>(
    ctx: &egui::Context,
    resp: &egui::Response,
    options: &[T],
    current: &mut T,
) -> bool {
    // 触控板像素滚动的跨帧累计量
    let acc_id = resp.id.with("wheel_acc");
    if !resp.hovered() {
        ctx.data_mut(|d| d.remove_temp::<f32>(acc_id));
        return false;
    }

    let mut steps = 0i32;
    let mut acc = ctx
        .data(|d| d.get_temp::<f32>(acc_id))
        .unwrap_or(0.0);
    const POINT_THRESHOLD: f32 = 40.0;

    ctx.input(|i| {
        for ev in &i.raw.events {
            if let egui::Event::MouseWheel { unit, delta, .. } = ev {
                let y = delta.y;
                if y == 0.0 {
                    continue;
                }
                match unit {
                    // 鼠标滚轮（行/页）：一个事件只走一格，即使系统一次给多行
                    egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => {
                        steps += if y > 0.0 { -1 } else { 1 };
                    }
                    // 触控板：累计像素，满阈值走一格，余量保留
                    egui::MouseWheelUnit::Point => {
                        acc += y;
                        while acc >= POINT_THRESHOLD {
                            acc -= POINT_THRESHOLD;
                            steps -= 1;
                        }
                        while acc <= -POINT_THRESHOLD {
                            acc += POINT_THRESHOLD;
                            steps += 1;
                        }
                    }
                }
            }
        }
    });
    ctx.data_mut(|d| d.insert_temp(acc_id, acc));

    if steps == 0 || options.is_empty() {
        return false;
    }
    let Some(idx) = options.iter().position(|o| o == current) else {
        return false;
    };
    // 边界停止，不循环
    let new_idx = (idx as i64 + steps as i64).clamp(0, options.len() as i64 - 1) as usize;
    if new_idx == idx {
        return false;
    }
    *current = options[new_idx];
    true
}

/// 中间页签。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CenterTab {
    Data,
    TableStat,
    EnvInfo,
}

/// 页内排序列（Key 列排序即全局遍历方向，由 sort_desc 表达，不在此列）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortCol {
    Index,
    Type,
    Value,
}

impl SortCol {
    pub fn label(self) -> &'static str {
        match self {
            SortCol::Index => "#",
            SortCol::Type => "类型",
            SortCol::Value => "Value",
        }
    }
}

/// 左侧表列表排序。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TableSort {
    NameAsc,
    NameDesc,
    CountAsc,
    CountDesc,
}

impl TableSort {
    pub const ALL: [TableSort; 4] = [
        TableSort::NameAsc,
        TableSort::NameDesc,
        TableSort::CountAsc,
        TableSort::CountDesc,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TableSort::NameAsc => "名称 ↑",
            TableSort::NameDesc => "名称 ↓",
            TableSort::CountAsc => "条数 ↑",
            TableSort::CountDesc => "条数 ↓",
        }
    }
}

pub struct MdbxerApp {
    // 顶栏
    pub path_input: String,
    pub open_mode: OpenMode,
    pub history: History,
    // 数据库
    pub db: Option<DbHandle>,
    // 左栏
    pub table_filter: String,
    pub table_sort: TableSort,
    pub left_visible: bool,
    // 中间
    pub tab: CenterTab,
    pub selected_table: Option<usize>,
    pub sort_desc: bool,
    /// 页内排序：(列, 升序?)。None = 表中读取出来的原始顺序
    pub col_sort: Option<(SortCol, bool)>,
    pub page_size: usize,
    pub rows: Vec<Row>,
    /// 当前页首行的估算绝对序号（跳转型导航后为 None）
    pub base_index: Option<usize>,
    pub at_start: bool,
    pub at_end: bool,
    pub selected_row: Option<usize>,
    pub jump_input: String,
    pub grid_mode: DecodeMode,
    /// 多字节整数的字节序（默认小端，可切大端）
    pub endian: crate::fmt::Endian,
    pub cell_max: usize,
    // 右栏
    pub detail_visible: bool,
    pub key_mode: DecodeMode,
    pub val_mode: DecodeMode,
    pub show_addr: bool,
    pub show_hex: bool,
    pub show_ascii: bool,
    pub hex_width: usize,
    pub dup_total: usize,
    pub dup_index: usize,
    pub dup_page_start: usize,
    pub dup_values: Vec<Vec<u8>>,
    /// 右侧多值：序号跳转输入（1 起）
    pub dup_jump_input: String,
    /// 右侧多值：值内容搜索输入（文本或 hex(...)）
    pub dup_search_input: String,
    // 右栏大字段分段查看（Key / Value 各自的字节偏移与跳转输入）
    pub key_seg_off: usize,
    pub val_seg_off: usize,
    pub key_seg_input: String,
    pub val_seg_input: String,
    // 页签缓存
    pub stat_cache: Option<(usize, Vec<(String, String)>)>,
    pub env_cache: Option<Vec<(String, String, String)>>,
    // 状态栏
    pub status: String,
}

impl MdbxerApp {
    pub fn new() -> Self {
        Self {
            path_input: String::new(),
            open_mode: OpenMode::Auto,
            history: History::load(),
            db: None,
            table_filter: String::new(),
            table_sort: TableSort::NameAsc,
            left_visible: true,
            tab: CenterTab::Data,
            selected_table: None,
            sort_desc: false,
            col_sort: None,
            page_size: DEFAULT_PAGE_SIZE,
            rows: Vec::new(),
            base_index: None,
            at_start: true,
            at_end: true,
            selected_row: None,
            jump_input: String::new(),
            grid_mode: DecodeMode::Auto,
            endian: crate::fmt::Endian::Little,
            cell_max: DEFAULT_CELL_MAX,
            key_mode: DecodeMode::Auto,
            val_mode: DecodeMode::Auto,
            show_addr: true,
            show_hex: true,
            show_ascii: true,
            hex_width: crate::fmt::DEFAULT_HEX_WIDTH,
            detail_visible: true,
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
            stat_cache: None,
            env_cache: None,
            status: "就绪".to_string(),
        }
    }

    pub fn cur_table(&self) -> Option<&TableInfo> {
        self.db
            .as_ref()
            .and_then(|dbh| self.selected_table.map(|i| &dbh.tables[i]))
    }

    /// 当前页的显示顺序（实际行索引列表）。未排序时即读取顺序 0..n。
    pub fn display_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.rows.len()).collect();
        let Some((col, asc)) = self.col_sort else { return order };
        match col {
            SortCol::Index => {}
            SortCol::Type => order.sort_by(|&a, &b| {
                crate::fmt::guess(&self.rows[a].value, self.endian)
                    .0
                    .cmp(&crate::fmt::guess(&self.rows[b].value, self.endian).0)
            }),
            SortCol::Value => order.sort_by(|&a, &b| {
                crate::fmt::decode(
                    &self.rows[a].value,
                    self.grid_mode,
                    self.endian,
                    self.cell_max,
                )
                .cmp(&crate::fmt::decode(
                    &self.rows[b].value,
                    self.grid_mode,
                    self.endian,
                    self.cell_max,
                ))
            }),
        }
        if !asc {
            order.reverse();
        }
        order
    }

    fn sort_dir(&self) -> Direction {
        if self.sort_desc {
            Direction::Backward
        } else {
            Direction::Forward
        }
    }

    // ── 打开 / 关闭 ─────────────────────────────────────────────

    pub fn open_db(&mut self) {
        let path = self.path_input.trim().trim_matches('"').to_string();
        if path.is_empty() {
            self.status = "请输入数据库路径".to_string();
            return;
        }
        match DbHandle::open(std::path::Path::new(&path), self.open_mode) {
            Ok(handle) => {
                let n = handle.tables.len();
                let mode_desc = if handle.no_sub_dir { "单文件" } else { "目录" };
                self.history.add(&path, self.open_mode.as_str());
                self.db = Some(handle);
                self.tab = CenterTab::Data;
                self.stat_cache = None;
                self.env_cache = None;
                self.selected_table = if n > 0 { Some(0) } else { None };
                self.status = format!("已打开（{mode_desc}模式，{n} 个表）：{path}");
                if self.selected_table.is_some() {
                    self.load_first_page();
                } else {
                    self.rows.clear();
                    self.base_index = None;
                    self.selected_row = None;
                    self.clear_dups();
                    self.status = format!("已打开但没有任何数据表：{path}");
                }
            }
            Err(e) => {
                self.status = e;
            }
        }
    }

    pub fn close_db(&mut self) {
        self.db = None;
        self.rows.clear();
        self.base_index = None;
        self.selected_table = None;
        self.selected_row = None;
        self.stat_cache = None;
        self.env_cache = None;
        self.clear_dups();
        self.status = "已关闭".to_string();
    }

    // ── 分页导航 ────────────────────────────────────────────────

    /// 取一页并写入 self.rows，返回 (页内条数, has_more)。
    fn fetch(
        &mut self,
        dir: Direction,
        anchor: Option<&Anchor>,
        skip_anchor: bool,
    ) -> Option<(usize, bool)> {
        let dbh = self.db.as_ref()?;
        let table = self.cur_table()?;
        let name = table.name.clone();
        let dup_sort = table.dup_sort;
        let page_size = self.page_size;
        match db::fetch_page(&dbh.db, name.as_deref(), dup_sort, dir, anchor, skip_anchor, page_size)
        {
            Ok(page) => {
                let len = page.rows.len();
                let has_more = page.has_more;
                self.rows = page.rows;
                Some((len, has_more))
            }
            Err(e) => {
                self.status = format!("读取失败：{e}");
                None
            }
        }
    }

    fn after_load(&mut self) {
        if self.rows.is_empty() {
            self.selected_row = None;
            self.clear_dups();
        } else {
            self.selected_row = Some(0);
            self.load_dups();
        }
    }

    pub fn select_table(&mut self, index: usize) {
        self.selected_table = Some(index);
        self.stat_cache = None;
        self.load_first_page();
    }

    pub fn load_first_page(&mut self) {
        let dir = self.sort_dir();
        if let Some((_len, has_more)) = self.fetch(dir, None, false) {
            self.at_start = true;
            self.at_end = !has_more;
            self.base_index = Some(0);
            self.after_load();
        }
    }

    pub fn load_last_page(&mut self) {
        let opp = opposite(self.sort_dir());
        if let Some((_len, has_more)) = self.fetch(opp, None, false) {
            self.rows.reverse();
            self.at_end = true;
            self.at_start = !has_more;
            let total = self.cur_table().map(|t| t.entries).unwrap_or(0);
            self.base_index = Some(total.saturating_sub(self.rows.len()));
            self.after_load();
        }
    }

    pub fn load_next_page(&mut self) {
        let Some(anchor) = self.rows.last().map(|r| self.anchor_of(r)) else {
            return;
        };
        let old_base = self.base_index;
        let old_len = self.rows.len();
        let dir = self.sort_dir();
        if let Some((_len, has_more)) = self.fetch(dir, Some(&anchor), true) {
            if self.rows.is_empty() {
                self.at_end = true;
                return;
            }
            self.at_start = false;
            self.at_end = !has_more;
            self.base_index = old_base.map(|b| b + old_len);
            self.after_load();
        }
    }

    pub fn load_prev_page(&mut self) {
        let Some(anchor) = self.rows.first().map(|r| self.anchor_of(r)) else {
            return;
        };
        let old_base = self.base_index;
        let page_size = self.page_size;
        let opp = opposite(self.sort_dir());
        if let Some((_len, has_more)) = self.fetch(opp, Some(&anchor), true) {
            if self.rows.is_empty() {
                self.at_start = true;
                return;
            }
            self.rows.reverse();
            self.at_start = !has_more;
            self.at_end = false;
            self.base_index = old_base.map(|b| b.saturating_sub(page_size));
            self.after_load();
        }
    }

    /// 行的锚点：多值表已按 Key 分组，锚点只需 Key。
    fn anchor_of(&self, row: &Row) -> Anchor {
        (row.key.clone(), None)
    }

    // ── 跳转 ────────────────────────────────────────────────────

    pub fn jump(&mut self) {
        let input = self.jump_input.trim().to_string();
        if input.is_empty() {
            return;
        }
        let integer_key = self.cur_table().map(|t| t.integer_key).unwrap_or(false);
        let key = match parse_jump_input(&input, integer_key) {
            Ok(k) => k,
            Err(e) => {
                self.status = format!("跳转输入错误：{e}");
                return;
            }
        };
        let Some(dbh) = self.db.as_ref() else { return };
        let Some(table) = self.cur_table() else { return };
        let name = table.name.clone();
        let dir = self.sort_dir();
        let page_size = self.page_size;
        match db::jump_to(&dbh.db, name.as_deref(), table.dup_sort, dir, key, page_size) {
            Ok(page) => {
                let found = !page.rows.is_empty();
                let has_more = page.has_more;
                self.rows = page.rows;
                self.at_start = false;
                self.at_end = !has_more;
                self.base_index = None;
                if found {
                    self.status = "已定位".to_string();
                } else {
                    self.status = "未找到不小于该 key 的记录".to_string();
                }
                self.after_load();
            }
            Err(e) => {
                self.status = format!("跳转失败：{e}");
            }
        }
    }

    // ── 多值 ────────────────────────────────────────────────────

    fn clear_dups(&mut self) {
        self.dup_total = 1;
        self.dup_index = 0;
        self.dup_page_start = 0;
        self.dup_values.clear();
        self.dup_jump_input.clear();
        self.dup_search_input.clear();
    }

    /// 选中行后加载右侧多值内容。多值表按 Key 分组显示，选中分组行
    /// 即从该 Key 的第一个值开始；值列表每页 DUP_PAGE_SIZE 个懒加载。
    fn load_dups(&mut self) {
        self.clear_dups();
        // 换了行：Key/Value 分段偏移都归零
        self.key_seg_off = 0;
        self.val_seg_off = 0;
        self.key_seg_input.clear();
        self.val_seg_input.clear();
        let Some(row) = self.selected_row.and_then(|i| self.rows.get(i)) else {
            return;
        };
        let Some(dbh) = self.db.as_ref() else { return };
        let Some(table) = self.cur_table() else { return };
        if !table.dup_sort {
            return;
        }
        let name = table.name.clone();
        if let Ok((total, values)) =
            db::dups_of(&dbh.db, name.as_deref(), &row.key, 0, DUP_PAGE_SIZE)
        {
            self.dup_total = total.max(1);
            self.dup_index = 0;
            self.dup_page_start = 0;
            self.dup_values = values;
        }
    }

    pub fn select_row(&mut self, index: usize) {
        if self.selected_row == Some(index) {
            return;
        }
        self.selected_row = Some(index);
        self.load_dups();
    }

    /// 跳转到当前 Key 的第 `idx` 个值（0 起）；自动夹到有效范围，跨值页时懒加载。
    pub fn dup_goto(&mut self, idx: usize) {
        if self.dup_total == 0 {
            return;
        }
        let idx = idx.min(self.dup_total - 1);
        let in_page = idx >= self.dup_page_start
            && idx < self.dup_page_start + self.dup_values.len();
        if !in_page {
            let Some(row) = self.selected_row.and_then(|i| self.rows.get(i)) else {
                return;
            };
            let key = row.key.clone();
            let Some(dbh) = self.db.as_ref() else { return };
            let Some(table) = self.cur_table() else { return };
            let page_index = idx / DUP_PAGE_SIZE;
            if let Ok((total, values)) = db::dups_of(
                &dbh.db,
                table.name.as_deref(),
                &key,
                page_index,
                DUP_PAGE_SIZE,
            ) {
                self.dup_total = total.max(1);
                self.dup_page_start = page_index * DUP_PAGE_SIZE;
                self.dup_values = values;
            }
        }
        self.dup_index = idx.min(self.dup_total.saturating_sub(1));
        // 切换到另一个值：Value 分段偏移归零
        self.val_seg_off = 0;
        self.val_seg_input.clear();
    }

    /// 上/下一个值（边界停止）。
    pub fn dup_step(&mut self, delta: isize) {
        let new = self.dup_index as isize + delta;
        if new < 0 || new >= self.dup_total as isize {
            return;
        }
        self.dup_goto(new as usize);
    }

    /// 上/下翻一个值页（DUP_PAGE_SIZE 个值），到头自动夹住。
    pub fn dup_page_step(&mut self, pages: isize) {
        let target = self.dup_index as isize + pages * DUP_PAGE_SIZE as isize;
        if target < 0 {
            self.dup_goto(0);
        } else {
            self.dup_goto(target as usize);
        }
    }

    /// 序号跳转：输入为 1 起的十进制序号。
    pub fn dup_jump(&mut self) {
        let s = self.dup_jump_input.trim();
        match s.parse::<usize>() {
            Ok(n) if n >= 1 && n <= self.dup_total => {
                self.dup_goto(n - 1);
                self.status = format!("已定位到第 {n}/{} 个值", self.dup_total);
            }
            Ok(n) => {
                self.status = format!("序号超出范围：{n}（共 {} 个值）", self.dup_total);
            }
            Err(_) => {
                self.status = "请输入有效的值序号（1 起的十进制数字）".to_string();
            }
        }
    }

    /// 在当前 Key 的值中按内容搜索：文本按 UTF-8，hex(...)/0x... 按字节；字节子串匹配。
    /// `forward=false` 向小序号方向查找；主方向无命中时回绕。
    pub fn dup_search(&mut self, forward: bool) {
        let s = self.dup_search_input.trim();
        if s.is_empty() {
            self.status = "请输入要搜索的值内容（文本或 hex(...)）".to_string();
            return;
        }
        let needle = match parse_bytes_input(s) {
            Ok(b) => b,
            Err(e) => {
                self.status = format!("搜索内容错误：{e}");
                return;
            }
        };
        let Some(row) = self.selected_row.and_then(|i| self.rows.get(i)) else {
            return;
        };
        let key = row.key.clone();
        let Some(dbh) = self.db.as_ref() else { return };
        let Some(table) = self.cur_table() else { return };
        // 向后从下一个值开始；向前从当前值之前开始
        let from = if forward { self.dup_index + 1 } else { self.dup_index };
        match db::dup_find(
            &dbh.db,
            table.name.as_deref(),
            &key,
            &needle,
            from,
            forward,
        ) {
            Ok(Some((i, _))) => {
                let wrapped = forward && i < from || !forward && i >= from;
                self.dup_goto(i);
                self.status = if wrapped {
                    format!("已回绕定位到第 {}/{} 个值", i + 1, self.dup_total)
                } else {
                    format!("已定位到第 {}/{} 个值", i + 1, self.dup_total)
                };
            }
            Ok(None) => {
                self.status = "当前 Key 的值中没有匹配内容".to_string();
            }
            Err(e) => {
                self.status = format!("搜索失败：{e}");
            }
        }
    }

    // ── 大字段分段查看 / 导出 ───────────────────────────────────

    pub fn seg_off(&self, is_key: bool) -> usize {
        if is_key { self.key_seg_off } else { self.val_seg_off }
    }

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
        let cur = (*off / crate::fmt::PAGE_BYTES) as isize;
        let max_seg = total.saturating_sub(1) / crate::fmt::PAGE_BYTES;
        let target = (cur + pages).clamp(0, max_seg as isize) as usize;
        *off = target * crate::fmt::PAGE_BYTES;
    }

    /// 跳至指定偏移：十进制或 0x 十六进制；向下对齐到段边界并夹到末尾段。
    pub fn seg_jump(&mut self, is_key: bool, total: usize) {
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
                *off = (v / crate::fmt::PAGE_BYTES) * crate::fmt::PAGE_BYTES;
                self.status = format!("已跳至偏移 {off}（0x{off:X}）");
            }
            Ok(v) => {
                self.status = format!("偏移超出范围：{v}（共 {total} 字节）");
            }
            Err(_) => {
                self.status = "请输入十进制偏移，或 0x 开头的十六进制偏移".to_string();
            }
        }
    }

    /// 把完整原始字节另存为文件（不经任何截断）；返回 true 表示已写出。
    pub fn save_bytes(&mut self, default_name: &str, bytes: &[u8]) -> bool {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(default_name)
            .add_filter("二进制", &["bin"])
            .add_filter("所有文件", &["*"])
            .save_file()
        else {
            return false;
        };
        match std::fs::write(&path, bytes) {
            Ok(()) => {
                self.status = format!("已导出 {} 字节到 {}", bytes.len(), path.display());
                true
            }
            Err(e) => {
                self.status = format!("导出失败：{e}");
                false
            }
        }
    }

    /// 当前应显示的 value：多值表取当前 dup，否则取选中行的 value。
    pub fn current_value(&self) -> Option<Vec<u8>> {
        let row = self.selected_row.and_then(|i| self.rows.get(i))?;
        let is_dup = self.cur_table().map(|t| t.dup_sort).unwrap_or(false);
        if is_dup {
            Some(
                self.dup_values
                    .get(self.dup_index.saturating_sub(self.dup_page_start))
                    .cloned()
                    .unwrap_or_else(|| row.value.clone()),
            )
        } else {
            Some(row.value.clone())
        }
    }
}

fn opposite(dir: Direction) -> Direction {
    match dir {
        Direction::Forward => Direction::Backward,
        Direction::Backward => Direction::Forward,
    }
}

/// 解析跳转输入：hex(...)/0x... → 字节；整数键表接受十进制；其余按 UTF-8 文本。
fn parse_jump_input(input: &str, integer_key: bool) -> Result<JumpKey, String> {
    let s = input.trim();
    let hex = s
        .strip_prefix("hex(")
        .and_then(|x| x.strip_suffix(')'))
        .or_else(|| s.strip_prefix("0x"))
        .or_else(|| s.strip_prefix("0X"));
    if let Some(h) = hex {
        return parse_hex(h).map(JumpKey::Bytes);
    }
    if integer_key {
        return s
            .parse::<u64>()
            .map(JumpKey::Int)
            .map_err(|_| "整数键表请输入十进制数字，或 hex(...)/0x... 形式的字节".to_string());
    }
    Ok(JumpKey::Bytes(s.as_bytes().to_vec()))
}

/// 通用字节输入解析：hex(...)/0x... → 十六进制字节；其余按 UTF-8 文本字节。
/// 供多值搜索等场景复用。
pub(crate) fn parse_bytes_input(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    let hex = s
        .strip_prefix("hex(")
        .and_then(|x| x.strip_suffix(')'))
        .or_else(|| s.strip_prefix("0x"))
        .or_else(|| s.strip_prefix("0X"));
    match hex {
        Some(h) => parse_hex(h),
        None => Ok(s.as_bytes().to_vec()),
    }
}

fn parse_hex(h: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = h.chars().filter(|c| !matches!(c, ' ' | '_' | ':')).collect();
    if cleaned.is_empty() || cleaned.len() % 2 != 0 {
        return Err("hex 长度必须为偶数".to_string());
    }
    (0..cleaned.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&cleaned[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

impl eframe::App for MdbxerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        // 拖拽文件/目录到窗口直接打开
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(file) = dropped.into_iter().next() {
            if let Some(path) = file.path().to_str() {
                self.path_input = path.to_string();
                self.open_mode = OpenMode::Auto;
                self.open_db();
            }
        }

        topbar::show(ui, self);
        if self.db.is_some() {
            if self.left_visible {
                sidebar::show(ui, self);
            }
            if self.detail_visible {
                detail::show(ui, self);
            }
        }

        // 底部状态栏
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                match (&self.db, self.cur_table()) {
                    (Some(dbh), Some(table)) => {
                        let range = if self.rows.is_empty() {
                            "0~0".to_string()
                        } else {
                            match self.base_index {
                                Some(b) => format!("{}~{}", b + 1, b + self.rows.len()),
                                None if table.dup_sort => {
                                    format!("本页 {} 个 Key", self.rows.len())
                                }
                                None => format!("本页 {} 条", self.rows.len()),
                            }
                        };
                        let total_desc = if table.dup_sort {
                            format!("共 {} 个值对", table.entries)
                        } else {
                            format!("共 {} 条", table.entries)
                        };
                        let range_desc = if table.dup_sort {
                            format!("第 {range} 个 Key")
                        } else {
                            format!("第 {range} 条")
                        };
                        ui.label(format!(
                            "{} — {range_desc} / {total_desc}",
                            table.display
                        ));
                        ui.separator();
                        ui.label(if dbh.no_sub_dir { "单文件模式" } else { "目录模式" });
                        ui.separator();
                        ui.label(format!("窗口 {} 条", self.page_size));
                    }
                    _ => {
                        ui.label("未打开数据库 — 输入路径，或将文件/目录拖入窗口");
                    }
                }
                ui.separator();
                ui.label(&self.status);
            });
        });

        // 中央区域
        egui::CentralPanel::default().show(ui, |ui| {
            if self.db.is_none() {
                ui.vertical_centered(|ui| {
                    ui.add_space(120.0);
                    ui.heading("MDBXer");
                    ui.label("libmdbx 数据库查看工具（只读）");
                    ui.label("请在上方输入数据库路径，或将文件/目录拖入窗口");
                });
                return;
            }
            let mut tab = self.tab;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut tab, CenterTab::Data, "数据");
                ui.selectable_value(&mut tab, CenterTab::TableStat, "表统计");
                ui.selectable_value(&mut tab, CenterTab::EnvInfo, "环境信息");
            });
            if tab != self.tab {
                self.tab = tab;
            }
            ui.separator();
            match self.tab {
                CenterTab::Data => dataview::show(ui, self),
                CenterTab::TableStat => statsview::show_table_stat(ui, self),
                CenterTab::EnvInfo => statsview::show_env_info(ui, self),
            }
        });

        // ↑/↓ 移动选中行（输入框聚焦时不抢键盘；按显示顺序移动）
        if !ui.ctx().egui_wants_keyboard_input() && !self.rows.is_empty() {
            let (up, down) = ui.ctx().input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::ArrowDown),
                )
            });
            let order = self.display_order();
            let cur_pos = self
                .selected_row
                .and_then(|r| order.iter().position(|&x| x == r))
                .unwrap_or(0);
            if down && cur_pos + 1 < order.len() {
                self.select_row(order[cur_pos + 1]);
            } else if up && cur_pos > 0 {
                self.select_row(order[cur_pos - 1]);
            }
        }
    }
}
