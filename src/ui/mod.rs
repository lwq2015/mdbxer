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
                crate::fmt::guess(&self.rows[a].value)
                    .0
                    .cmp(crate::fmt::guess(&self.rows[b].value).0)
            }),
            SortCol::Value => order.sort_by(|&a, &b| {
                crate::fmt::decode(&self.rows[a].value, self.grid_mode, self.cell_max).cmp(
                    &crate::fmt::decode(&self.rows[b].value, self.grid_mode, self.cell_max),
                )
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

    /// 行的锚点：多值表带上 value 以精确定位到 (key, value) 对。
    fn anchor_of(&self, row: &Row) -> Anchor {
        let dup = self.cur_table().map(|t| t.dup_sort).unwrap_or(false);
        (
            row.key.clone(),
            if dup { Some(row.value.clone()) } else { None },
        )
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
        match db::jump_to(&dbh.db, name.as_deref(), dir, key, page_size) {
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
    }

    fn load_dups(&mut self) {
        self.clear_dups();
        let Some(row) = self.selected_row.and_then(|i| self.rows.get(i)) else {
            return;
        };
        let key = row.key.clone();
        let value = row.value.clone();
        let Some(dbh) = self.db.as_ref() else { return };
        let Some(table) = self.cur_table() else { return };
        if !table.dup_sort {
            return;
        }
        let name = table.name.clone();
        if let Ok(Some(idx)) = db::dup_index_of(&dbh.db, name.as_deref(), &key, &value) {
            self.dup_index = idx;
        }
        let page_index = self.dup_index / DUP_PAGE_SIZE;
        if let Ok((total, values)) =
            db::dups_of(&dbh.db, name.as_deref(), &key, page_index, DUP_PAGE_SIZE)
        {
            self.dup_total = total.max(1);
            self.dup_page_start = page_index * DUP_PAGE_SIZE;
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

    pub fn dup_step(&mut self, delta: isize) {
        let new = self.dup_index as isize + delta;
        if new < 0 || new >= self.dup_total as isize {
            return;
        }
        self.dup_index = new as usize;
        if self.dup_index < self.dup_page_start
            || self.dup_index >= self.dup_page_start + DUP_PAGE_SIZE
        {
            let Some(row) = self.selected_row.and_then(|i| self.rows.get(i)) else {
                return;
            };
            let key = row.key.clone();
            let Some(dbh) = self.db.as_ref() else { return };
            let Some(table) = self.cur_table() else { return };
            let page_index = self.dup_index / DUP_PAGE_SIZE;
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
                                None => format!("本页 {} 条", self.rows.len()),
                            }
                        };
                        ui.label(format!(
                            "{} — 第 {range} / 共 {} 条",
                            table.display, table.entries
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
