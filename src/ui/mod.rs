// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 界面层：应用状态与导航动作。只接触 db 层暴露的纯数据结构。

mod about;
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
/// 导出时 UI 线程每批读取的原始 KV 条数（一帧一批，兼顾流畅与吞吐）
pub const EXPORT_BATCH: usize = 2000;

/// 左栏宽度范围（点）
pub const LEFT_PANEL_MIN: f32 = 180.0;
pub const LEFT_PANEL_MAX: f32 = 320.0;
/// 中央数据表保留的最小宽度：两侧面板拖宽时不得把它挤得更窄
pub const MIDDLE_MIN_WIDTH: f32 = 360.0;

/// 状态栏消息。
/// 持久状态（就绪/已打开/已关闭等）每帧按当前语言渲染，切换语言即时跟随；
/// 一次性提示（错误、跳转结果等）保留生成时的文本，下次操作自然被替换。
pub enum Status {
    /// 就绪（启动初始态）
    Ready,
    /// 已打开：是否单文件模式、表数量、路径
    Opened {
        file_mode: bool,
        n: usize,
        path: String,
    },
    /// 已打开但库中没有任何表
    NoTables(String),
    /// 已关闭
    Closed,
    /// 一次性消息（沿用生成时的语言）
    Msg(String),
}

impl Status {
    /// 按当前语言渲染为状态栏文本。
    pub fn text(&self) -> String {
        let t = crate::i18n::tr();
        match self {
            Status::Ready => t.ready.to_string(),
            Status::Opened { file_mode, n, path } => {
                let mode = if *file_mode { t.m_file } else { t.m_dir };
                t.open_ok(mode, *n, path)
            }
            Status::NoTables(path) => t.open_no_tables(path),
            Status::Closed => t.closed.to_string(),
            Status::Msg(s) => s.clone(),
        }
    }
}

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
    let mut acc = ctx.data(|d| d.get_temp::<f32>(acc_id)).unwrap_or(0.0);
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
    /// 关于（版本/环境/链接/致谢/免责）
    About,
}

impl CenterTab {
    /// 页签名（随界面语言）。
    pub fn label(self) -> &'static str {
        let t = crate::i18n::tr();
        match self {
            CenterTab::Data => t.tab_data,
            CenterTab::TableStat => t.tab_stat,
            CenterTab::EnvInfo => t.tab_env,
            CenterTab::About => t.tab_about,
        }
    }
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
    /// 列头标题（随界面语言；"#"/"Value" 三种语言通用）。
    pub fn label(self) -> String {
        match self {
            SortCol::Index => "#".to_string(),
            SortCol::Type => crate::i18n::tr().col_type.to_string(),
            SortCol::Value => "Value".to_string(),
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

    /// 排序下拉标签（随界面语言）。
    pub fn label(self) -> String {
        let t = crate::i18n::tr();
        match self {
            TableSort::NameAsc => t.sort_name_asc.to_string(),
            TableSort::NameDesc => t.sort_name_desc.to_string(),
            TableSort::CountAsc => t.sort_count_asc.to_string(),
            TableSort::CountDesc => t.sort_count_desc.to_string(),
        }
    }

    /// 持久化标识（与语言无关）。
    pub fn as_str(self) -> &'static str {
        match self {
            TableSort::NameAsc => "name_asc",
            TableSort::NameDesc => "name_desc",
            TableSort::CountAsc => "count_asc",
            TableSort::CountDesc => "count_desc",
        }
    }

    /// 从持久化字符串解析；未知值回退名称升序。
    pub fn from_prefs(s: &str) -> TableSort {
        match s {
            "name_desc" => TableSort::NameDesc,
            "count_asc" => TableSort::CountAsc,
            "count_desc" => TableSort::CountDesc,
            _ => TableSort::NameAsc,
        }
    }
}

/// 全表 Value 搜索的运行状态。
struct ValueSearchState {
    /// 搜索词（小写，匹配时也将目标转小写比较）
    needle: String,
    /// 下一批续读锚点（None = 从头开始）
    anchor: Option<db::RawAnchor>,
    /// 已扫描条数（进度用）
    checked: usize,
}

pub struct MdbxerApp {
    // ── 顶栏 ──
    /// 打开模式（自动/单文件/目录）
    pub open_mode: OpenMode,
    /// 历史记录（持久化到 %APPDATA%）
    pub history: History,
    // ── 数据库 ──
    /// 当前打开的 MDBX 环境；None 表示未打开
    pub db: Option<DbHandle>,
    /// 当前打开的数据库路径（用于窗口标题）；None 表示未打开
    pub opened_path: Option<String>,
    // ── 左栏 ──
    /// 表名过滤输入框
    pub table_filter: String,
    /// 表列表排序方式
    pub table_sort: TableSort,
    /// 左栏（表列表）是否显示
    pub left_visible: bool,
    /// 左栏当前宽度（上一帧实测，用于约束右栏上限；默认 220）
    pub left_panel_w: f32,
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
    /// 共享搜索输入框内容（Key 跳转/过滤、Value 页内过滤共用）
    pub search_input: String,
    /// 当前生效的 Key 前缀过滤（None = 未过滤）
    pub key_filter: Option<Vec<u8>>,
    /// Key 搜索模式：false = 跳转定位；true = 前缀过滤
    pub key_filter_mode: bool,
    /// 当前生效的 Value 页内过滤文本（None = 未过滤）
    pub value_filter: Option<String>,
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
    /// 缓存生成时的解码参数 (key_mode, val_mode, endian, cell_max, 千位分隔, 语言)；None = 需重建
    views_key: Option<(DecodeMode, DecodeMode, crate::fmt::Endian, usize, bool, u8)>,
    // ── 右栏 ──
    /// 右栏（详情）是否显示
    pub detail_visible: bool,
    /// 右栏详情状态（hex 视图配置 / 多值导航 / 大字段分段查看）
    pub detail: DetailState,
    /// 右栏当前宽度（上一帧实测，用于约束左栏上限；默认 360）
    pub detail_panel_w: f32,
    // ── 页签缓存 ──
    /// 表统计缓存：(表下标, 行数据)
    pub stat_cache: Option<(usize, Vec<(String, String)>)>,
    /// 环境信息缓存
    pub env_cache: Option<Vec<(String, String, String)>>,
    // ── 状态栏 ──
    /// 状态栏消息（持久状态随语言即时渲染）
    pub status: Status,
    // ── 导出 ──
    /// worker 事件接收端（Some = 导出进行中，也用于禁用导出按钮防重入）
    pub export_ev_rx: Option<std::sync::mpsc::Receiver<crate::export::ExportEvent>>,
    /// 给 worker 发送数据批/中止的发送端
    export_batch_tx: Option<std::sync::mpsc::Sender<crate::export::ExportBatch>>,
    /// 导出下一批的续读锚点（None = 首批）
    export_anchor: Option<db::RawAnchor>,
    /// 导出已写出条数（进度显示，UI 每发出一批即累加）
    pub export_count: usize,
    /// 导出目标文件路径（完成消息用）
    pub export_path: String,
    /// 导出格式选择
    pub export_format: crate::export::ExportFormat,
    // ── 全表 Value 搜索 ──
    /// 全表 Value 搜索状态（Some = 搜索进行中）
    value_search: Option<ValueSearchState>,
    /// Ctrl+F 请求聚焦搜索框（下一帧渲染到搜索框时消费，避免直接
    /// request_focus 未知 ID 导致 accesskit panic）
    focus_search: bool,
    /// 搜索框的真实控件 ID（每帧更新，用于判断 Ctrl+C 是否在复制框内文本）
    search_box_id: Option<egui::Id>,
    /// 行复制武装标记：点击表格行后置位，搜索框重新聚焦时清除。
    /// 点击行不会转移 egui 焦点（搜索框可能仍持有焦点），所以用显式
    /// 标记而不是焦点判断来决定 Ctrl+C 的归属。
    row_copy_pending: bool,
    /// Value 搜索的全表模式开关（默认关闭：V 键仅过滤当前页；
    /// 开启后 V 键全表扫描并定位）
    pub value_search_full: bool,
    // ── 主题 / 收藏 ──
    /// 当前主题（true = 深色）
    pub dark_theme: bool,
    /// 当前库的收藏表（None 元素 = 主表），左栏置顶显示
    pub fav_tables: Vec<Option<String>>,
    /// 当前库的收藏 Key，左栏底部列表可跳转
    pub fav_keys: Vec<crate::config::FavKey>,
    // ── 窗口标题 ──
    /// 基础标题（版本+日期，启动时由 main 传入）
    title_base: String,
    /// 当前已应用的窗口标题（含库路径），用于变化检测
    title: String,
    /// 构建日期（取 exe 修改时间，"关于"页展示；取不到为 "—"）
    build_date: String,
}

impl MdbxerApp {
    /// 新建应用状态：加载历史记录、主题与 UI 偏好，其余字段取默认值。
    /// `title_base` 为基础窗口标题（版本+日期），打开库后前缀库路径。
    pub fn new(title_base: String) -> Self {
        let prefs = crate::config::load_ui_prefs();
        let page_size = prefs
            .page_size
            .filter(|p| PAGE_SIZES.contains(p))
            .unwrap_or(DEFAULT_PAGE_SIZE);
        let endian = match prefs.endian_le {
            Some(false) => crate::fmt::Endian::Big,
            _ => crate::fmt::Endian::Little,
        };
        let key_mode = prefs
            .key_mode
            .map(|s| DecodeMode::from_str(&s))
            .unwrap_or_default();
        let val_mode = prefs
            .val_mode
            .map(|s| DecodeMode::from_str(&s))
            .unwrap_or_default();
        if let Some(ts) = prefs.thousands_sep {
            crate::fmt::set_thousands_sep(ts);
        }
        let cell_max = prefs
            .cell_max
            .filter(|v| [64usize, 128, 256, 512, 1024, 4096].contains(v))
            .unwrap_or(DEFAULT_CELL_MAX);
        let hex_width = prefs
            .hex_width
            .filter(|w| crate::fmt::HEX_WIDTHS.contains(w))
            .unwrap_or(crate::fmt::DEFAULT_HEX_WIDTH);
        let mut detail = DetailState::default();
        detail.show_addr = prefs.show_addr.unwrap_or(true);
        detail.show_hex = prefs.show_hex.unwrap_or(true);
        detail.show_ascii = prefs.show_ascii.unwrap_or(true);
        detail.hex_width = hex_width;
        detail.key_mode = key_mode;
        detail.val_mode = val_mode;
        let table_sort = prefs
            .table_sort
            .as_deref()
            .map(TableSort::from_prefs)
            .unwrap_or(TableSort::NameAsc);
        let export_format = prefs
            .export_format
            .as_deref()
            .and_then(|s| {
                crate::export::ExportFormat::ALL
                    .into_iter()
                    .find(|f| f.ext() == s)
            })
            .unwrap_or_default();
        Self {
            open_mode: OpenMode::Auto,
            history: History::load(),
            db: None,
            opened_path: None,
            table_filter: String::new(),
            table_sort,
            left_visible: prefs.left_visible.unwrap_or(true),
            left_panel_w: 220.0,
            tab: CenterTab::Data,
            selected_table: None,
            sort_desc: false,
            col_sort: None,
            page_size,
            rows: Vec::new(),
            base_index: None,
            at_start: true,
            at_end: true,
            selected_row: None,
            search_input: String::new(),
            key_filter: None,
            key_filter_mode: false,
            value_filter: None,
            key_mode,
            val_mode,
            endian,
            cell_max,
            views: Vec::new(),
            views_key: None,
            detail_visible: prefs.detail_visible.unwrap_or(true),
            detail,
            detail_panel_w: 360.0,
            stat_cache: None,
            env_cache: None,
            status: Status::Ready,
            export_ev_rx: None,
            export_batch_tx: None,
            export_anchor: None,
            export_count: 0,
            export_path: String::new(),
            export_format,
            value_search: None,
            focus_search: false,
            search_box_id: None,
            row_copy_pending: false,
            value_search_full: false,
            dark_theme: crate::config::load_theme() == crate::config::Theme::Dark,
            fav_tables: Vec::new(),
            fav_keys: Vec::new(),
            title: title_base.clone(),
            title_base,
            build_date: build_date().unwrap_or_else(|| "—".to_string()),
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
        // Value 页内搜索：按 Value 显示文本大小写不敏感包含过滤
        if let Some(filter) = &self.value_filter {
            let needle = filter.to_lowercase();
            order.retain(|&i| self.views[i].val_text.to_lowercase().contains(&needle));
        }
        let Some((col, asc)) = self.col_sort else {
            return order;
        };
        match col {
            SortCol::Index => {}
            SortCol::Type => {
                order.sort_by(|&a, &b| self.views[a].type_label.cmp(&self.views[b].type_label))
            }
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
            crate::i18n::lang() as u8,
        );
        if self.views_key == Some(key) && self.views.len() == self.rows.len() {
            return;
        }
        self.views = self
            .rows
            .iter()
            .map(|r| {
                let val_text =
                    crate::fmt::decode(&r.value, self.val_mode, self.endian, self.cell_max);
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

    /// 打开 `path` 指向的数据库；成功后加载第一页并把路径写入窗口标题。
    /// 路径统一规范化为绝对路径（剥掉 Windows `\\?\` 前缀）再用于显示与持久化，
    /// 保证从任意工作目录启动都能命中每库记忆。
    pub fn open_db(&mut self, path: &str) {
        let path = path.trim().trim_matches('"').to_string();
        if path.is_empty() {
            return;
        }
        match DbHandle::open(std::path::Path::new(&path), self.open_mode) {
            Ok(handle) => {
                // 打开成功后规范化路径（canonicalize 要求路径存在，故放在此处）
                let path = normalize_path(&path);
                let n = handle.tables.len();
                let file_mode = handle.no_sub_dir;
                self.history.add(&path, self.open_mode.as_str());
                // 每库记录：恢复收藏与最后打开的表（表已被删则回退主表）
                let rec = crate::config::load_per_db(&path);
                self.fav_tables = rec.fav_tables.clone();
                self.fav_keys = rec.fav_keys.clone();
                self.selected_table = if n > 0 {
                    rec.last_table
                        .and_then(|name| {
                            handle
                                .tables
                                .iter()
                                .position(|t| t.name.as_ref() == Some(&name))
                        })
                        .or(Some(0))
                } else {
                    None
                };
                self.db = Some(handle);
                self.opened_path = Some(path.clone());
                self.tab = CenterTab::Data;
                self.stat_cache = None;
                self.env_cache = None;
                self.search_input.clear();
                self.key_filter = None;
                self.value_filter = None;
                self.status = Status::Opened {
                    file_mode,
                    n,
                    path: path.clone(),
                };
                // 刷新 LRU 的 last_use
                self.save_per_db();
                if self.selected_table.is_some() {
                    self.load_first_page();
                } else {
                    self.rows.clear();
                    self.base_index = None;
                    self.selected_row = None;
                    self.detail.clear();
                    self.status = Status::NoTables(path);
                }
            }
            Err(e) => {
                // 路径已不存在：自动从历史移除（相对路径先尽力绝对化以便匹配）。
                // 其他失败（占用、格式不符等）可能是暂时的，保留历史。
                if !std::path::Path::new(&path).exists() {
                    let abs = absolutize_path(&path);
                    self.history.remove_path(&abs);
                }
                self.status = Status::Msg(e);
            }
        }
    }

    /// 关闭当前数据库并清空所有相关状态；窗口标题恢复为基础标题。
    pub fn close_db(&mut self) {
        // 关库前保存最后打开的表与收藏
        self.save_per_db();
        // 导出流水线依赖本线程持有的环境句柄：断开通道，worker 自然结束
        self.export_batch_tx = None;
        self.export_ev_rx = None;
        self.export_anchor = None;
        self.value_search = None;
        self.db = None;
        self.opened_path = None;
        self.rows.clear();
        self.base_index = None;
        self.selected_table = None;
        self.selected_row = None;
        self.stat_cache = None;
        self.env_cache = None;
        self.search_input.clear();
        self.key_filter = None;
        self.value_filter = None;
        self.fav_tables.clear();
        self.fav_keys.clear();
        self.detail.clear();
        self.status = Status::Closed;
    }

    /// 检测 `opened_path` 变化，同步更新窗口标题。
    fn update_title(&mut self, ctx: &egui::Context) {
        let want = match &self.opened_path {
            Some(p) => format!("{} · {p}", self.title_base),
            None => self.title_base.clone(),
        };
        if want != self.title {
            self.title = want.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(want));
        }
    }

    /// 启动时重新打开上次查看的库（按每库记录 LRU 取最近一条）。
    /// 路径已不存在则静默跳过；无记录时不做任何事。
    pub fn reopen_last_db(&mut self) {
        let Some(path) = crate::config::last_opened_db() else {
            return;
        };
        if !std::path::Path::new(&path).exists() {
            // 上次的库已不在：顺手清理失效历史项，避免下拉里残留
            self.history.remove_path(&path);
            return;
        }
        self.open_mode = OpenMode::Auto;
        self.open_db(&path);
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
        let prefix = self.key_filter.clone();
        match db::fetch_page_prefix(
            &dbh.db,
            name.as_deref(),
            dup_sort,
            dir,
            prefix.as_deref(),
            anchor,
            skip_anchor,
            page_size,
        ) {
            Ok(page) => {
                let len = page.rows.len();
                let has_more = page.has_more;
                self.rows = page.rows;
                Some((len, has_more))
            }
            Err(e) => {
                self.status = Status::Msg(crate::i18n::tr().read_fail(&e));
                None
            }
        }
    }

    /// 页面加载完成后：选中首行（或清空选中）并加载右侧多值。
    fn after_load(&mut self) {
        // rows 可能换了新数据（即使长度相同），强制重建 views 缓存
        self.views_key = None;
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
        self.search_input.clear();
        self.key_filter = None;
        self.value_filter = None;
        self.value_search = None;
        self.load_first_page();
        self.save_per_db();
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

    /// 按给定输入执行 Key 跳转（Key 搜索框跳转模式调用，不修改输入框内容）。
    fn jump_with(&mut self, input: &str) {
        if input.is_empty() {
            return;
        }
        let integer_key = self.cur_table().map(|t| t.integer_key).unwrap_or(false);
        let key = match parse_jump_input(&input, integer_key) {
            Ok(k) => k,
            Err(e) => {
                self.status = Status::Msg(crate::i18n::tr().jump_bad(&e));
                return;
            }
        };
        // 跳转是在全表任意定位，与前缀过滤互斥：跳转即清除过滤
        self.key_filter = None;
        let Some(dbh) = self.db.as_ref() else { return };
        let Some(table) = self.cur_table() else {
            return;
        };
        let name = table.name.clone();
        let dir = self.sort_dir();
        let page_size = self.page_size;
        match db::jump_to(
            &dbh.db,
            name.as_deref(),
            table.dup_sort,
            dir,
            key,
            page_size,
        ) {
            Ok(page) => {
                let found = !page.rows.is_empty();
                let has_more = page.has_more;
                self.rows = page.rows;
                self.at_start = false;
                self.at_end = !has_more;
                self.base_index = None;
                if found {
                    self.status = Status::Msg(crate::i18n::tr().located.to_string());
                } else {
                    self.status = Status::Msg(crate::i18n::tr().not_found_ge.to_string());
                }
                self.after_load();
            }
            Err(e) => {
                self.status = Status::Msg(crate::i18n::tr().jump_fail(&e));
            }
        }
    }

    /// Key 搜索：跳转模式复用 [`Self::jump`] 定位；前缀过滤模式只显示以输入
    /// 开头的 Key（文本或 hex(...)/0x... 输入，按字节语义匹配）。
    pub fn apply_key_search(&mut self) {
        let input = self.search_input.trim().to_string();
        if input.is_empty() {
            return;
        }
        if !self.key_filter_mode {
            // 跳转模式：语义与跳转框一致（INTEGER_KEY 表接受十进制），但不写跳转框
            self.jump_with(&input);
            return;
        }
        match parse_bytes_input(&input) {
            Ok(bytes) => {
                self.key_filter = Some(bytes);
                self.load_first_page();
            }
            Err(e) => {
                self.status = Status::Msg(crate::i18n::tr().key_search_bad(&e));
            }
        }
    }

    /// 清除 Key 前缀过滤并回到全表首页。
    pub fn clear_key_search(&mut self) {
        self.key_filter = None;
        self.search_input.clear();
        self.load_first_page();
    }

    /// Value 页内搜索：把当前输入框内容作为 Value 显示文本的包含过滤条件。
    /// 空输入清除过滤。
    pub fn apply_value_search(&mut self) {
        let input = self.search_input.trim().to_string();
        self.value_filter = if input.is_empty() { None } else { Some(input) };
    }

    /// 清除 Value 页内过滤（不清空输入框）。
    pub fn clear_value_search(&mut self) {
        self.value_filter = None;
    }

    /// 全局快捷键：Ctrl+O 打开、Ctrl+F 聚焦搜索框、Ctrl+C 复制选中行、
    /// Esc 清除过滤、PgUp/PgDn 翻页。
    pub fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // Ctrl+C/X/V 在 egui-winit 层被转换为 Event::Copy/Cut/Paste 后直接消费，
        // 不会再产生 Key::C 按键事件，因此复制只能监听 Event::Copy/Cut。
        // TextEdit/可选 Label 稍后处理同一事件：有文本选区时会覆盖剪贴板，
        // 无选区时不动，于是"先复制行、选区后覆盖"的优先级天然正确。
        let copy_event = ctx.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Copy | egui::Event::Cut))
        });
        if copy_event {
            // 复制选中行，除非搜索框刚被聚焦（此时让 egui 处理框内
            // 选中文本复制）。点击表格行会重新武装行复制。
            if self.row_copy_pending || ctx.memory(|m| m.focused().is_none()) {
                self.copy_selected_row(ctx);
            }
            self.row_copy_pending = false;
        }

        let (ctrl, key) = ctx.input(|i| {
            let ctrl = i.modifiers.command;
            let mut key = None;
            for k in [
                egui::Key::O,
                egui::Key::F,
                egui::Key::Escape,
                egui::Key::PageUp,
                egui::Key::PageDown,
            ] {
                if i.key_pressed(k) {
                    key = Some(k);
                    break;
                }
            }
            (ctrl, key)
        });
        let Some(key) = key else { return };

        match key {
            egui::Key::O if ctrl => {
                // 与顶栏"文件"按钮一致：弹出文件选择框（目录请用"目录"按钮）
                let path = rfd::FileDialog::new()
                    .add_filter("MDBX", &["mdbx", "dat", "*"])
                    .pick_file()
                    .map(|p| p.display().to_string());
                if let Some(p) = path {
                    self.open_mode = OpenMode::SingleFile;
                    self.open_db(&p);
                }
            }
            egui::Key::F if ctrl => {
                // 只设标记，搜索框渲染时再 request_focus：
                // 直接对猜测的 ID request_focus 会因 ID 不存在触发 accesskit panic
                self.focus_search = true;
            }
            egui::Key::Escape => {
                self.clear_key_search();
                self.clear_value_search();
                self.cancel_value_search();
            }
            egui::Key::PageUp if !ctrl => {
                self.load_prev_page();
            }
            egui::Key::PageDown if !ctrl => {
                self.load_next_page();
            }
            _ => {}
        }
    }

    /// 复制选中行的 Key 和 Value 到剪贴板（Tab 分隔）。
    /// 用完整解码（65536 字符上限），不受表格单元格截断影响。
    fn copy_selected_row(&mut self, ctx: &egui::Context) {
        let Some(i) = self.selected_row else { return };
        let Some(row) = self.rows.get(i) else { return };
        let key_text = crate::fmt::decode(&row.key, self.key_mode, self.endian, 65536);
        let val_text = crate::fmt::decode(&row.value, self.val_mode, self.endian, 65536);
        let text = format!("{}\t{}", key_text, val_text);
        ctx.copy_text(text);
        self.status = Status::Msg(crate::i18n::tr().copied_row.to_string());
    }

    /// 开始全表 Value 搜索：逐批扫描全表，找到第一个 Value 显示文本包含
    /// 搜索词的记录并定位到该行。搜索在 UI 线程分批执行，不阻塞界面。
    pub fn start_full_value_search(&mut self) {
        let needle = self.search_input.trim().to_string();
        if needle.is_empty() {
            self.status = Status::Msg(crate::i18n::tr().search_empty.to_string());
            return;
        }
        // 全表搜索结果自带定位，旧的页内过滤会干扰显示，一并清除
        self.value_filter = None;
        self.value_search = Some(ValueSearchState {
            needle: needle.to_lowercase(),
            anchor: None,
            checked: 0,
        });
        self.status = Status::Msg(crate::i18n::tr().searching_value.to_string());
    }

    /// 取消进行中的全表 Value 搜索。
    pub fn cancel_value_search(&mut self) {
        self.value_search = None;
    }

    /// 每帧驱动全表 Value 搜索：读一批、检查匹配、找到则定位。
    pub fn poll_value_search(&mut self) {
        let Some(mut state) = self.value_search.take() else {
            return;
        };
        let Some(dbh) = self.db.as_ref() else { return };
        let Some(table) = self.cur_table() else {
            return;
        };
        let name = table.name.clone();
        let dup_sort = table.dup_sort;
        let dir = self.sort_dir();
        let needle = state.needle.clone();
        let anchor = state.anchor.clone();
        let val_mode = self.val_mode;
        let endian = self.endian;
        let page_size = self.page_size;

        const BATCH: usize = 500;
        let batch = match db::fetch_raw_batch(
            &dbh.db,
            name.as_deref(),
            dup_sort,
            dir,
            anchor.as_ref(),
            anchor.is_some(),
            BATCH,
        ) {
            Ok(b) => b,
            Err(e) => {
                self.status = Status::Msg(e);
                return;
            }
        };

        let mut found: Option<(Vec<u8>, Vec<u8>)> = None;
        for (k, v) in &batch.rows {
            // 同时按 Auto 解码与当前排版解码匹配：当前排版为 hex/数值时
            // 文本搜索仍可用 Auto 命中
            let auto_text =
                crate::fmt::decode(v, crate::fmt::DecodeMode::Auto, endian, 65536).to_lowercase();
            if auto_text.contains(&needle) {
                found = Some((k.clone(), v.clone()));
                break;
            }
            let mode_text = crate::fmt::decode(v, val_mode, endian, 65536).to_lowercase();
            if mode_text.contains(&needle) {
                found = Some((k.clone(), v.clone()));
                break;
            }
        }
        state.checked += batch.rows.len();

        if let Some((k, _v)) = found {
            // 定位：复用 jump_to（普通表落到匹配 key 的行，多值表落到该 key
            // 的分组页，保持分组显示不破坏）
            let page = db::jump_to(
                &dbh.db,
                name.as_deref(),
                dup_sort,
                dir,
                db::JumpKey::Bytes(k),
                page_size,
            );
            match page {
                Ok(p) => {
                    let located = !p.rows.is_empty();
                    self.rows = p.rows;
                    self.at_start = false;
                    self.at_end = !p.has_more;
                    self.base_index = None;
                    self.after_load();
                    self.status = Status::Msg(if located {
                        crate::i18n::tr().value_found.to_string()
                    } else {
                        crate::i18n::tr().value_not_found.to_string()
                    });
                }
                Err(e) => {
                    self.status = Status::Msg(e);
                }
            }
            return;
        }

        // 未找到：续读或结束
        if batch.has_more {
            state.anchor = batch.rows.last().cloned();
            self.status = Status::Msg(crate::i18n::tr().searching_value_progress(state.checked));
            self.value_search = Some(state);
        } else {
            self.status = Status::Msg(crate::i18n::tr().value_not_found.to_string());
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
                    .get(
                        self.detail
                            .dup_index
                            .saturating_sub(self.detail.dup_page_start),
                    )
                    .cloned()
                    .unwrap_or_else(|| row.value.clone()),
            )
        } else {
            Some(row.value.clone())
        }
    }

    /// 把完整原始字节另存为文件（不经任何截断）；返回 true 表示已写出。
    pub fn save_bytes(&mut self, default_name: &str, bytes: &[u8]) -> bool {
        let t = crate::i18n::tr();
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(default_name)
            .add_filter(t.filter_binary, &["bin"])
            .add_filter(t.filter_all, &["*"])
            .save_file()
        else {
            return false;
        };
        match std::fs::write(&path, bytes) {
            Ok(()) => {
                self.status = Status::Msg(t.export_ok(bytes.len(), &path.display().to_string()));
                true
            }
            Err(e) => {
                self.status = Status::Msg(t.export_fail(&e.to_string()));
                false
            }
        }
    }

    // ── 导出 ────────────────────────────────────────────────────

    /// 启动后台导出（防重入：若已有导出在运行则忽略）。
    /// worker 只写文件；数据批由本线程（持有唯一环境句柄）按请求供给。
    pub fn start_export(&mut self) {
        if self.export_ev_rx.is_some() {
            return;
        }
        let t = crate::i18n::tr();
        let Some(table) = self.cur_table() else {
            return;
        };
        let name = table.display();
        let ext = self.export_format.ext();
        let Some(out) = rfd::FileDialog::new()
            .set_file_name(format!(
                "{}.{ext}",
                name.replace(|c: char| !c.is_alphanumeric(), "_")
            ))
            .add_filter(t.filter_all, &["*"])
            .save_file()
        else {
            return;
        };
        let job = crate::export::ExportJob {
            dup_sort: table.dup_sort,
            key_mode: self.key_mode,
            val_mode: self.val_mode,
            endian: self.endian,
            out_path: out.clone(),
            format: self.export_format,
        };
        let (batch_tx, event_rx) = crate::export::start(job);
        self.export_batch_tx = Some(batch_tx);
        self.export_ev_rx = Some(event_rx);
        self.export_anchor = None;
        self.export_count = 0;
        self.export_path = out.display().to_string();
        self.status = Status::Msg(t.export_started.to_string());
    }

    /// 从当前表读取导出用的下一批原始 KV（多值表逐值展开）。
    fn read_export_batch(&self) -> Result<db::RawBatch, String> {
        let dbh = self.db.as_ref().ok_or("db closed")?;
        let table = self.cur_table().ok_or("no table")?;
        db::fetch_raw_batch(
            &dbh.db,
            table.name.as_deref(),
            table.dup_sort,
            self.sort_dir(),
            self.export_anchor.as_ref(),
            self.export_anchor.is_some(),
            EXPORT_BATCH,
        )
    }

    /// 每帧轮询导出事件并按需供给数据批（需在 eframe::App::ui 里调用）。
    pub fn poll_export(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.export_ev_rx.take() else {
            return;
        };
        let t = crate::i18n::tr();
        let mut finished = false;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                crate::export::ExportEvent::NeedBatch => {
                    match self.read_export_batch() {
                        Ok(batch) => {
                            let db::RawBatch { rows, has_more } = batch;
                            self.export_anchor = rows.last().cloned();
                            self.export_count += rows.len();
                            let Some(tx) = &self.export_batch_tx else {
                                finished = true;
                                break;
                            };
                            if tx
                                .send(crate::export::ExportBatch::Rows { rows, has_more })
                                .is_err()
                            {
                                finished = true;
                                break;
                            }
                            if has_more {
                                self.status = Status::Msg(t.export_progress(self.export_count));
                            }
                        }
                        Err(e) => {
                            // 读批失败：通知 worker 中止并清理
                            if let Some(tx) = &self.export_batch_tx {
                                let _ = tx.send(crate::export::ExportBatch::Abort(e.clone()));
                            }
                            self.status = Status::Msg(t.export_fail(&e));
                            finished = true;
                            break;
                        }
                    }
                }
                crate::export::ExportEvent::Done(n) => {
                    self.export_count = n;
                    self.status = Status::Msg(t.export_done(n, &self.export_path));
                    finished = true;
                }
                crate::export::ExportEvent::Fail(e) => {
                    self.status = Status::Msg(t.export_fail(&e));
                    finished = true;
                }
            }
        }
        if finished {
            self.export_batch_tx = None;
            self.export_anchor = None;
        } else {
            self.export_ev_rx = Some(rx);
            // 导出仍在运行，请求下一帧重绘以持续轮询
            ctx.request_repaint();
        }
    }

    // ── 主题 / UI 偏好 / 收藏 ───────────────────────────────────

    /// 切换深浅色主题并持久化。
    pub fn toggle_theme(&mut self, ctx: &egui::Context) {
        self.dark_theme = !self.dark_theme;
        apply_theme(ctx, self.dark_theme);
        crate::config::save_theme(if self.dark_theme {
            crate::config::Theme::Dark
        } else {
            crate::config::Theme::Light
        });
    }

    /// 当前 UI 偏好（页大小/字节序/排版/千位分隔），供持久化。
    pub fn current_ui_prefs(&self) -> crate::config::UiPrefs {
        crate::config::UiPrefs {
            page_size: Some(self.page_size),
            endian_le: Some(self.endian == crate::fmt::Endian::Little),
            key_mode: Some(self.key_mode.as_str().to_string()),
            val_mode: Some(self.val_mode.as_str().to_string()),
            thousands_sep: Some(crate::fmt::thousands_sep()),
            cell_max: Some(self.cell_max),
            show_addr: Some(self.detail.show_addr),
            show_hex: Some(self.detail.show_hex),
            show_ascii: Some(self.detail.show_ascii),
            hex_width: Some(self.detail.hex_width),
            left_visible: Some(self.left_visible),
            detail_visible: Some(self.detail_visible),
            table_sort: Some(self.table_sort.as_str().to_string()),
            export_format: Some(self.export_format.ext().to_string()),
        }
    }

    /// 保存当前 UI 偏好（各设置变更点调用）。
    pub fn save_ui_prefs(&self) {
        crate::config::save_ui_prefs(&self.current_ui_prefs());
    }

    /// 保存当前库的每库记录（最后打开的表 + 收藏）。未打开库时无操作。
    fn save_per_db(&self) {
        let Some(path) = &self.opened_path else {
            return;
        };
        crate::config::save_per_db(&crate::config::PerDbRecord {
            path: path.clone(),
            last_table: self.cur_table().and_then(|t| t.name.clone()),
            fav_tables: self.fav_tables.clone(),
            fav_keys: self.fav_keys.clone(),
            ..Default::default()
        });
    }

    /// 切换某表的收藏状态（None = 主表）。
    pub fn toggle_fav_table(&mut self, name: Option<String>) {
        if let Some(pos) = self.fav_tables.iter().position(|n| *n == name) {
            self.fav_tables.remove(pos);
        } else {
            self.fav_tables.push(name);
        }
        self.save_per_db();
    }

    /// 切换当前表中某 Key 的收藏状态。
    pub fn toggle_fav_key(&mut self, key: &[u8]) {
        let Some(table) = self.cur_table() else {
            return;
        };
        let table_name = table.name.clone();
        let key_hex = key_hex(key);
        if let Some(pos) = self
            .fav_keys
            .iter()
            .position(|f| f.table == table_name && f.key_hex == key_hex)
        {
            self.fav_keys.remove(pos);
        } else {
            self.fav_keys.push(crate::config::FavKey {
                table: table_name,
                key_hex,
                note: String::new(),
            });
        }
        self.save_per_db();
    }

    /// 当前表的该 Key 是否已收藏。
    pub fn is_fav_key(&self, key: &[u8]) -> bool {
        let Some(table) = self.cur_table() else {
            return false;
        };
        let hex = key_hex(key);
        self.fav_keys
            .iter()
            .any(|f| f.table == table.name && f.key_hex == hex)
    }

    /// 删除收藏 Key 列表中的第 `index` 条。
    pub fn remove_fav_key(&mut self, index: usize) {
        if index < self.fav_keys.len() {
            self.fav_keys.remove(index);
            self.save_per_db();
        }
    }

    /// 跳转到收藏 Key：先切到目标表，再复用 Key 搜索的跳转定位。
    pub fn jump_to_fav(&mut self, fk: &crate::config::FavKey) {
        let Some(idx) = self
            .db
            .as_ref()
            .and_then(|dbh| dbh.tables.iter().position(|t| t.name == fk.table))
        else {
            // 表已不存在：移除失效收藏
            self.fav_keys.retain(|f| f != fk);
            self.save_per_db();
            return;
        };
        self.select_table(idx);
        self.search_input = format!("hex({})", fk.key_hex);
        self.key_filter_mode = false;
        self.apply_key_search();
    }
}

/// 应用主题到 egui 上下文。
pub fn apply_theme(ctx: &egui::Context, dark: bool) {
    ctx.set_visuals(if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    });
}

/// 字节的大写 hex 串（收藏 Key 的持久化形式）。
fn key_hex(key: &[u8]) -> String {
    key.iter().map(|b| format!("{b:02X}")).collect()
}

/// 路径规范化为绝对形式；剥掉 Windows canonicalize 的 verbatim 前缀
/// （`\\?\C:\…` → `C:\…`、`\\?\UNC\srv\share` → `\\srv\share`），便于显示。
/// canonicalize 失败时退回原样（相对路径照常用）。
fn normalize_path(path: &str) -> String {
    let p = std::fs::canonicalize(path).unwrap_or_else(|_| std::path::PathBuf::from(path));
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.into_owned()
    }
}

/// 尽力把路径转为绝对形式（不要求路径存在，故不能用 canonicalize）。
/// 用于打开失败时与历史记录（均为 canonicalize 绝对路径）匹配。
fn absolutize_path(path: &str) -> String {
    let p = std::path::Path::new(path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|d| d.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    };
    let s = abs.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.into_owned()
    }
}

/// 构建日期：取 exe 自身修改时间（即本次构建/发布时间），无需 build.rs。
pub(crate) fn build_date() -> Option<String> {
    std::env::current_exe()
        .and_then(|p| std::fs::metadata(p))
        .and_then(|m| m.modified())
        .ok()
        .map(|t| {
            let dt: chrono::DateTime<chrono::Local> = t.into();
            dt.format("%Y-%m-%d").to_string()
        })
}

/// 反转取值方向（供"上一页"等反向操作使用）。
fn opposite(dir: Direction) -> Direction {
    match dir {
        Direction::Forward => Direction::Backward,
        Direction::Backward => Direction::Forward,
    }
}

/// 从 Value 显示文本提取数值供排序：整体可解析（"42"/"2.5"/"1e20"/"1,234"），
/// 或去掉补零后缀、" → 日期"注释后可解析时返回 Some；否则 None 按文本排序。
fn leading_num(s: &str) -> Option<f64> {
    // 先剥掉千位分隔逗号，再按纯数字解析
    let cleaned: String = s.trim().replace(',', "");
    let t = cleaned.as_str();
    if let Ok(v) = t.parse::<f64>() {
        return Some(v);
    }
    if let Some(p) = t.strip_suffix(crate::i18n::tr().padded) {
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
            .map_err(|_| crate::i18n::tr().int_key_hint.to_string());
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
    let cleaned: String = h
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | ':'))
        .collect();
    if cleaned.is_empty() || cleaned.len() % 2 != 0 {
        return Err(crate::i18n::tr().hex_even.to_string());
    }
    (0..cleaned.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&cleaned[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

impl MdbxerApp {
    /// 底部状态栏（整行宽度）：表名/范围/条数、打开模式、每页条数、持久状态消息。
    fn show_status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            let t = crate::i18n::tr();
            ui.horizontal(|ui| {
                match (&self.db, self.cur_table()) {
                    (Some(dbh), Some(table)) => {
                        let range = if self.rows.is_empty() {
                            "0~0".to_string()
                        } else {
                            match self.base_index {
                                Some(b) => format!("{}~{}", b + 1, b + self.rows.len()),
                                None if table.dup_sort => t.page_keys(self.rows.len()),
                                None => t.page_rows(self.rows.len()),
                            }
                        };
                        let total_desc = if table.dup_sort {
                            t.total_pairs(table.entries)
                        } else {
                            t.total_rows(table.entries)
                        };
                        let range_desc = if table.dup_sort {
                            t.range_keys(&range)
                        } else {
                            t.range_rows(&range)
                        };
                        ui.label(format!("{} — {range_desc} / {total_desc}", table.display()));
                        ui.separator();
                        ui.label(if dbh.no_sub_dir {
                            t.mode_file
                        } else {
                            t.mode_dir
                        });
                        ui.separator();
                        ui.label(t.window_size(self.page_size));
                    }
                    _ => {
                        ui.label(t.no_db_status);
                    }
                }
                ui.separator();
                ui.label(self.status.text());
            });
        });
    }
}

impl eframe::App for MdbxerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        // 拖拽文件/目录到窗口直接打开
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(file) = dropped.into_iter().next() {
            if let Some(path) = file.path().to_str() {
                self.open_mode = OpenMode::Auto;
                self.open_db(path);
            }
        }

        self.update_title(ctx);
        self.poll_export(ctx);
        self.poll_value_search();
        self.handle_shortcuts(ctx);

        topbar::show(ui, self);
        // 底部状态栏先于左右栏声明：egui 面板按声明顺序从可用区域切割，
        // 先声明的 bottom 占满整行宽度，左右栏随后落在顶栏与状态栏之间。
        self.show_status_bar(ui);
        if self.db.is_some() {
            if self.left_visible {
                sidebar::show(ui, self);
            }
            if self.detail_visible {
                detail::show(ui, self);
            }
        }

        // 中央区域（页签始终显示；"关于"无库也可查看）
        egui::CentralPanel::default().show(ui, |ui| {
            let t = crate::i18n::tr();
            let mut tab = self.tab;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut tab, CenterTab::Data, CenterTab::Data.label());
                ui.selectable_value(&mut tab, CenterTab::TableStat, CenterTab::TableStat.label());
                ui.selectable_value(&mut tab, CenterTab::EnvInfo, CenterTab::EnvInfo.label());
                ui.selectable_value(&mut tab, CenterTab::About, CenterTab::About.label());
            });
            if tab != self.tab {
                self.tab = tab;
            }
            ui.separator();
            if self.tab == CenterTab::About {
                let date = self.build_date.clone();
                about::show(ui, &date);
                return;
            }
            if self.db.is_none() {
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.heading("MDBXer");
                    ui.label(t.subtitle);
                    ui.label(t.no_db_center);
                });
                return;
            }
            match self.tab {
                CenterTab::Data => dataview::show(ui, self),
                CenterTab::TableStat => statsview::show_table_stat(ui, self),
                CenterTab::EnvInfo => statsview::show_env_info(ui, self),
                CenterTab::About => unreachable!(),
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
