//! 界面层：应用状态与导航动作。只接触 db 层暴露的纯数据结构。

mod dataview;
mod detail;
mod sidebar;
mod statsview;
mod topbar;

pub use detail::DetailState;

use crate::db::{self, Anchor, DbHandle, Direction, JumpKey, OpenMode, Row, TableInfo};
use crate::fmt::DecodeMode;
use crate::history::History;

pub const PAGE_SIZES: [usize; 5] = [50, 100, 200, 500, 1000];
pub const DEFAULT_PAGE_SIZE: usize = 200;
pub const DEFAULT_CELL_MAX: usize = 256;

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
    /// 数据浏览（表格 + 翻页 + 跳转）
    Data,
    /// 表统计（B+树/页分布/标志）
    TableStat,
    /// 环境信息（几何/映射/事务/读者/主表）
    EnvInfo,
}

/// 页内排序列（Key 列排序即全局遍历方向，由 sort_desc 表达，不在此列）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortCol {
    /// 绝对序号列
    Index,
    /// 类型猜测列
    Type,
    /// Value 列
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

/// 一行的显示缓存：渲染与页内排序共用，避免每帧重复解码。
pub struct RowView {
    /// Key 的显示文本（按当前表格解码格式）
    pub key_text: String,
    /// Value 的显示文本（按当前表格解码格式）
    pub val_text: String,
    /// Value 的类型猜测标签
    pub type_label: String,
    /// val_text 开头能解析出的数值（数值排序用；hex/文本等为 None）
    pub val_num: Option<f64>,
}

/// 左侧表列表排序。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TableSort {
    /// 名称升序
    NameAsc,
    /// 名称降序
    NameDesc,
    /// 条数升序
    CountAsc,
    /// 条数降序
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
    // ── 顶栏 ──
    /// 数据库路径输入框内容
    pub path_input: String,
    /// 打开模式（自动/单文件/目录）
    pub open_mode: OpenMode,
    /// 历史记录（持久化到 %APPDATA%）
    pub history: History,
    // ── 数据库 ──
    /// 当前打开的 MDBX 环境；None 表示未打开
    pub db: Option<DbHandle>,
    // ── 左栏 ──
    /// 表名过滤输入框
    pub table_filter: String,
    /// 表列表排序方式
    pub table_sort: TableSort,
    /// 左栏（表列表）是否显示
    pub left_visible: bool,
    // ── 中间 ──
    /// 当前页签
    pub tab: CenterTab,
    /// 选中的表在 dbh.tables 中的下标
    pub selected_table: Option<usize>,
    /// 全局遍历方向（true = 降序，即 Key 列排序）
    pub sort_desc: bool,
    /// 页内排序：(列, 升序?)。None = 表中读取出来的原始顺序
    pub col_sort: Option<(SortCol, bool)>,
    /// 每页条数（用户可选 50/100/200/500/1000）
    pub page_size: usize,
    /// 当前页数据行
    pub rows: Vec<Row>,
    /// 当前页首行的估算绝对序号（跳转型导航后为 None）
    pub base_index: Option<usize>,
    /// 已在取值方向的最前
    pub at_start: bool,
    /// 已在取值方向的最后
    pub at_end: bool,
    /// 选中的行在 self.rows 中的下标
    pub selected_row: Option<usize>,
    /// Key 跳转输入框内容
    pub jump_input: String,
    /// Key 列解码格式（默认自动；编码固定的表可手动指定）
    pub key_mode: DecodeMode,
    /// Value 列解码格式（默认自动：Value 逐行猜测）
    pub val_mode: DecodeMode,
    /// 多字节整数的字节序（默认小端，可切大端）
    pub endian: crate::fmt::Endian,
    /// 单元格最多显示字符数（超出截断）
    pub cell_max: usize,
    /// 当前页显示缓存（与 rows 一一对应），由 display_order 按需重建
    pub views: Vec<RowView>,
    /// 缓存生成时的解码参数 (key_mode, val_mode, endian, cell_max, 千位分隔)；None = 需重建
    views_key: Option<(DecodeMode, DecodeMode, crate::fmt::Endian, usize, bool)>,
    // ── 右栏 ──
    /// 右栏（详情）是否显示
    pub detail_visible: bool,
    /// 右栏详情状态（hex 视图配置 / 多值导航 / 大字段分段查看）
    pub detail: DetailState,
    // ── 页签缓存 ──
    /// 表统计缓存：(表下标, 行数据)
    pub stat_cache: Option<(usize, Vec<(String, String)>)>,
    /// 环境信息缓存
    pub env_cache: Option<Vec<(String, String, String)>>,
    // ── 状态栏 ──
    /// 状态栏文本
    pub status: String,
}

impl MdbxerApp {
    /// 新建应用状态：加载历史记录，其余字段取默认值。
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
            key_mode: DecodeMode::Auto,
            val_mode: DecodeMode::Auto,
            endian: crate::fmt::Endian::Little,
            cell_max: DEFAULT_CELL_MAX,
            views: Vec::new(),
            views_key: None,
            detail_visible: true,
            detail: DetailState::default(),
            stat_cache: None,
            env_cache: None,
            status: "就绪".to_string(),
        }
    }

    /// 当前选中的表信息（未打开库或未选表时为 None）。
    pub fn cur_table(&self) -> Option<&TableInfo> {
        self.db
            .as_ref()
            .and_then(|dbh| self.selected_table.map(|i| &dbh.tables[i]))
    }

    /// 当前页的显示顺序（实际行索引列表）。未排序时即读取顺序 0..n。
    /// 页内排序使用 views 缓存的显示文本：Value 列数值按数值比较，其余按文本。
    pub fn display_order(&mut self) -> Vec<usize> {
        self.refresh_views();
        let mut order: Vec<usize> = (0..self.rows.len()).collect();
        let Some((col, asc)) = self.col_sort else { return order };
        match col {
            SortCol::Index => {}
            SortCol::Type => order.sort_by(|&a, &b| {
                self.views[a].type_label.cmp(&self.views[b].type_label)
            }),
            SortCol::Value => order.sort_by(|&a, &b| {
                let (va, vb) = (&self.views[a], &self.views[b]);
                match (va.val_num, vb.val_num) {
                    // 两边都是数值：按数值比（避免 "10" < "9" 的字典序问题）
                    (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                    // 数值排在文本前
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => va.val_text.cmp(&vb.val_text),
                }
            }),
        }
        if !asc {
            order.reverse();
        }
        order
    }

    /// 确保 views 显示缓存与 rows + 当前解码参数一致；任一变化时整页重建。
    fn refresh_views(&mut self) {
        let key = (
            self.key_mode,
            self.val_mode,
            self.endian,
            self.cell_max,
            crate::fmt::thousands_sep(),
        );
        if self.views_key == Some(key) && self.views.len() == self.rows.len() {
            return;
        }
        self.views = self
            .rows
            .iter()
            .map(|r| {
                let val_text = crate::fmt::decode(&r.value, self.val_mode, self.endian, self.cell_max);
                RowView {
                    key_text: crate::fmt::decode(&r.key, self.key_mode, self.endian, self.cell_max),
                    type_label: crate::fmt::guess(&r.value, self.endian).0,
                    val_num: leading_num(&val_text),
                    val_text,
                }
            })
            .collect();
        self.views_key = Some(key);
    }

    /// 全局遍历方向（升序/降序）转为 db 层的 Direction。
    fn sort_dir(&self) -> Direction {
        if self.sort_desc {
            Direction::Backward
        } else {
            Direction::Forward
        }
    }

    // ── 打开 / 关闭 ─────────────────────────────────────────────

    /// 打开 `path_input` 指向的数据库；成功后加载第一页。
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
                    self.detail.clear();
                    self.status = format!("已打开但没有任何数据表：{path}");
                }
            }
            Err(e) => {
                self.status = e;
            }
        }
    }

    /// 关闭当前数据库并清空所有相关状态。
    pub fn close_db(&mut self) {
        self.db = None;
        self.rows.clear();
        self.base_index = None;
        self.selected_table = None;
        self.selected_row = None;
        self.stat_cache = None;
        self.env_cache = None;
        self.detail.clear();
        self.status = "已关闭".to_string();
    }

    // ── 分页导航 ────────────────────────────────────────────────

    /// 取一页并写入 self.rows，返回 (页内条数, has_more)。
    /// 错误信息写入 status 并返回 None。
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

    /// 页面加载完成后：选中首行（或清空选中）并加载右侧多值。
    fn after_load(&mut self) {
        if self.rows.is_empty() {
            self.selected_row = None;
            self.detail.clear();
        } else {
            self.selected_row = Some(0);
            self.load_dups();
        }
    }

    /// 切换选中表并加载第一页。
    pub fn select_table(&mut self, index: usize) {
        self.selected_table = Some(index);
        self.stat_cache = None;
        self.load_first_page();
    }

    /// 首页（取值方向最前）。
    pub fn load_first_page(&mut self) {
        let dir = self.sort_dir();
        if let Some((_len, has_more)) = self.fetch(dir, None, false) {
            self.at_start = true;
            self.at_end = !has_more;
            self.base_index = Some(0);
            self.after_load();
        }
    }

    /// 末页（取值方向最后）。
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

    /// 下一页（沿当前遍历方向）。
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

    /// 上一页（沿当前遍历方向的反方向）。
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

    /// Key 跳转（跳转型导航，base_index 置 None）。
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

    /// 选中行后加载右侧多值内容。多值表按 Key 分组显示，选中分组行
    /// 即从该 Key 的第一个值开始；值列表每页 DUP_PAGE_SIZE 个懒加载。
    fn load_dups(&mut self) {
        let Some(row) = self.selected_row.and_then(|i| self.rows.get(i)) else {
            self.detail.clear();
            return;
        };
        let key = row.key.clone();
        let Some(table) = self.cur_table() else {
            self.detail.clear();
            return;
        };
        let (name, dup_sort) = (table.name.clone(), table.dup_sort);
        let Some(dbh) = self.db.as_ref() else {
            self.detail.clear();
            return;
        };
        self.detail
            .load_dups(&dbh.db, name.as_deref(), dup_sort, &key);
    }

    /// 选中一行并加载其多值（普通表无操作）。
    pub fn select_row(&mut self, index: usize) {
        if self.selected_row == Some(index) {
            return;
        }
        self.selected_row = Some(index);
        self.load_dups();
    }

    /// 当前应显示的 value：多值表取当前 dup，否则取选中行的 value。
    pub fn current_value(&self) -> Option<Vec<u8>> {
        let row = self.selected_row.and_then(|i| self.rows.get(i))?;
        let is_dup = self.cur_table().map(|t| t.dup_sort).unwrap_or(false);
        if is_dup {
            Some(
                self.detail
                    .dup_values
                    .get(self.detail.dup_index.saturating_sub(self.detail.dup_page_start))
                    .cloned()
                    .unwrap_or_else(|| row.value.clone()),
            )
        } else {
            Some(row.value.clone())
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
}

/// 反转取值方向（供"上一页"等反向操作使用）。
fn opposite(dir: Direction) -> Direction {
    match dir {
        Direction::Forward => Direction::Backward,
        Direction::Backward => Direction::Forward,
    }
}

/// 从 Value 显示文本提取数值供排序：整体可解析（"42"/"2.5"/"1e20"/"1,234"），
/// 或去掉"（补零）"后缀、" → 日期"注释后可解析时返回 Some；否则 None 按文本排序。
fn leading_num(s: &str) -> Option<f64> {
    // 先剥掉千位分隔逗号，再按纯数字解析
    let cleaned: String = s.trim().replace(',', "");
    let t = cleaned.as_str();
    if let Ok(v) = t.parse::<f64>() {
        return Some(v);
    }
    if let Some(p) = t.strip_suffix("（补零）") {
        if let Ok(v) = p.trim().parse::<f64>() {
            return Some(v);
        }
    }
    if let Some((p, _)) = t.split_once(" → ") {
        if let Ok(v) = p.trim().parse::<f64>() {
            return Some(v);
        }
    }
    None
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

/// 解析十六进制文本为字节：忽略空格/下划线/冒号，长度须为偶数。
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
