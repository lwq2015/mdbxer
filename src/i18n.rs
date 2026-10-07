// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 界面多语言（中文 / English / Русский）。
//!
//! 零依赖方案：编译期三张静态字符串表 + 全局原子语言状态。
//! 取词用 [`tr`]：`tr().btn_file`；带变量的句子走 `I18n` 上的方法
//! （模板含 `{name}` 占位符，允许各语言调整语序）。
//! 语言切换即时生效（egui 即时模式每帧重建）。

use std::sync::atomic::{AtomicU8, Ordering};

/// 支持的界面语言。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Lang {
    /// 中文
    Zh = 0,
    /// English
    En = 1,
    /// Русский（向 libmdbx 及其作者 Leonid Yuriev 致敬）
    Ru = 2,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::Zh, Lang::En, Lang::Ru];

    /// 持久化用代码：zh / en / ru。
    pub fn code(self) -> &'static str {
        match self {
            Lang::Zh => "zh",
            Lang::En => "en",
            Lang::Ru => "ru",
        }
    }

    /// 从持久化代码解析；未知值回退到中文。
    pub fn from_code(s: &str) -> Lang {
        match s {
            "en" => Lang::En,
            "ru" => Lang::Ru,
            _ => Lang::Zh,
        }
    }

    /// 下拉框中用各自语言显示的名称。
    pub fn label(self) -> &'static str {
        match self {
            Lang::Zh => "中文",
            Lang::En => "English",
            Lang::Ru => "Русский",
        }
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);

/// 当前界面语言。
pub fn lang() -> Lang {
    match LANG.load(Ordering::Relaxed) {
        1 => Lang::En,
        2 => Lang::Ru,
        _ => Lang::Zh,
    }
}

/// 切换界面语言（下一帧全部界面文本即生效）。
pub fn set_lang(l: Lang) {
    LANG.store(l as u8, Ordering::Relaxed);
}

/// 当前语言的字符串表。
pub fn tr() -> &'static I18n {
    &TABLES[lang() as usize]
}

/// 模板替换：把 `{xxx}` 占位符替换为实际值。
fn fill(tpl: &str, pairs: &[(&str, &str)]) -> String {
    let mut s = tpl.to_string();
    for (k, v) in pairs {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

/// 一张语言的全部界面字符串。字段名按界面区域分组。
#[derive(Clone, Copy)]
pub struct I18n {
    // ── 通用状态 ──
    pub ready: &'static str,
    pub closed: &'static str,

    // ── 顶栏 ──
    pub btn_file: &'static str,
    pub btn_dir: &'static str,
    pub btn_close: &'static str,
    pub btn_reload_tip: &'static str,
    pub history: &'static str,
    pub history_empty: &'static str,
    pub history_del_tip: &'static str,
    pub endian: &'static str,
    pub endian_le: &'static str,
    pub endian_be: &'static str,
    pub cell_max_tip: &'static str,
    pub thousands_tip: &'static str,
    pub panel_tables: &'static str,
    pub panel_detail: &'static str,

    // ── 打开模式（状态栏显示用） ──
    pub mode_file: &'static str,
    pub mode_dir: &'static str,

    // ── 解码格式 ──
    pub mode_auto: &'static str,

    // ── 左栏 ──
    pub tables_title: &'static str,
    pub filter_hint: &'static str,
    pub sort_name_asc: &'static str,
    pub sort_name_desc: &'static str,
    pub sort_count_asc: &'static str,
    pub sort_count_desc: &'static str,
    /// 表行模板：{name} {n} {flag}
    pub table_entry_t: &'static str,
    /// 有标志描述时的前缀（中文用逗号，其余语言 ", "）
    pub table_flag_sep: &'static str,

    // ── 中央页签 / 空态 ──
    pub tab_data: &'static str,
    pub tab_stat: &'static str,
    pub tab_env: &'static str,
    pub tab_about: &'static str,
    pub subtitle: &'static str,
    pub select_table: &'static str,

    // ── 数据页工具条/表格 ──
    pub default_order: &'static str,
    pub sort_hint: &'static str,
    pub reset_order: &'static str,
    pub tip_first: &'static str,
    pub tip_prev: &'static str,
    pub tip_next: &'static str,
    pub tip_last: &'static str,
    /// 每页条数下拉前的短标签
    pub page_size_label: &'static str,
    pub page_size_tip: &'static str,
    pub col_type: &'static str,
    /// Key 列头 hover：说明是全表遍历方向排序
    pub col_tip_key: &'static str,
    /// #/类型/Value 列头 hover：说明仅页内排序
    pub col_tip_page: &'static str,
    /// 多值分组行后缀模板 {n}
    pub dup_n_values_t: &'static str,

    // ── 右侧详情 ──
    pub detail_select_hint: &'static str,
    pub hex_addr_tip: &'static str,
    pub addr: &'static str,
    pub hex_col_tip: &'static str,
    pub hex_ascii_tip: &'static str,
    pub hex_width_tip: &'static str,
    pub copy: &'static str,
    pub copied_row: &'static str,
    pub save_as: &'static str,
    pub save_tip: &'static str,
    pub seg_prev_tip: &'static str,
    pub seg_next_tip: &'static str,
    pub seg_input_hint: &'static str,
    /// 段信息模板 {a} {b} {off} {total} {hex}
    pub seg_line_t: &'static str,
    /// Value 卡片多值标题模板 {i} {n}
    pub val_title_t: &'static str,
    pub dup_tip_first: &'static str,
    pub dup_tip_prev100: &'static str,
    pub dup_tip_prev: &'static str,
    pub dup_tip_next: &'static str,
    pub dup_tip_next100: &'static str,
    pub dup_tip_last: &'static str,
    pub goto: &'static str,
    pub search_hint: &'static str,
    pub search_prev_tip: &'static str,
    pub search_next_tip: &'static str,
    /// 猜测行模板 {label} {n}
    pub guess_line_t: &'static str,
    /// 字节数模板 {n}
    pub bytes_t: &'static str,
    pub section_text: &'static str,
    pub section_hex: &'static str,
    /// 交互式 hex 视图操作提示（悬停整个区域时显示）
    pub hex_hint: &'static str,

    // ── 多值/分段状态消息（模板与提示）──
    pub dup_located_t: &'static str,
    pub dup_wrap_t: &'static str,
    pub dup_range_t: &'static str,
    pub dup_bad_num: &'static str,
    pub dup_prompt: &'static str,
    pub dup_bad_query_t: &'static str,
    pub dup_nomatch: &'static str,
    pub dup_fail_t: &'static str,
    pub dup_load_fail_t: &'static str,
    pub seg_ok_t: &'static str,
    pub seg_range_t: &'static str,
    pub seg_bad: &'static str,
    /// 大字段内字节搜索
    pub blob_prompt: &'static str,
    pub blob_nomatch: &'static str,
    pub blob_located_t: &'static str,
    pub blob_wrap_t: &'static str,

    // ── 搜索工具条（Key/Value 共享输入框） ──
    pub toolbar_search_hint: &'static str,
    /// Key 搜索模式开关 hover 提示（按当前状态分：提示将切换到哪个模式）
    pub key_mode_tip_off: &'static str,
    pub key_mode_tip_on: &'static str,
    /// "K" 按钮 hover 提示
    pub key_search_btn_tip: &'static str,
    /// "V" 按钮 hover 提示（按当前模式分状态）
    pub value_search_tip_off: &'static str,
    pub value_search_tip_on: &'static str,
    pub full_search_toggle: &'static str,
    /// "全表"复选框 hover 提示（按当前状态分：提示将切换到哪个模式）
    pub full_search_tip_off: &'static str,
    pub full_search_tip_on: &'static str,
    pub search_empty: &'static str,
    pub filter_active: &'static str,
    pub filter_clear_tip: &'static str,
    /// {e}
    pub key_search_bad_t: &'static str,

    // ── 导出 ──
    pub export_tip: &'static str,
    pub export_started: &'static str,
    /// {n}
    pub export_progress_t: &'static str,
    /// {n} {path}
    pub export_done_t: &'static str,

    // ── 主题 / 收藏 ──
    /// 深色主题下：切换到浅色
    pub theme_to_light: &'static str,
    /// 浅色主题下：切换到深色
    pub theme_to_dark: &'static str,
    pub favorites_title: &'static str,
    pub fav_add_t_tip: &'static str,
    pub fav_rm_t_tip: &'static str,
    pub fav_add_k_tip: &'static str,
    pub fav_rm_k_tip: &'static str,
    pub fav_jump_tip: &'static str,
    pub fav_empty: &'static str,

    // ── 应用状态消息（模板）──
    /// {mode} {n} {path}
    pub open_ok_t: &'static str,
    /// {path}
    pub open_no_tables_t: &'static str,
    /// {e}
    pub read_fail_t: &'static str,
    /// {e}
    pub jump_bad_t: &'static str,
    pub located: &'static str,
    pub not_found_ge: &'static str,
    pub searching_value: &'static str,
    /// {n}
    pub searching_value_progress_t: &'static str,
    pub value_found: &'static str,
    pub value_not_found: &'static str,
    /// {e}
    pub jump_fail_t: &'static str,
    /// {n} {path}
    pub export_ok_t: &'static str,
    /// {e}
    pub export_fail_t: &'static str,
    /// {e}
    pub stat_fail_t: &'static str,
    /// {e}
    pub env_fail_t: &'static str,

    // ── 状态栏 ──
    /// {n}
    pub page_keys_t: &'static str,
    pub page_rows_t: &'static str,
    pub total_pairs_t: &'static str,
    pub total_rows_t: &'static str,
    /// {range}
    pub range_keys_t: &'static str,
    pub range_rows_t: &'static str,
    /// {n}
    pub window_size_t: &'static str,
    pub no_db_status: &'static str,

    // ── 空态中央提示（可拖放）──
    pub no_db_center: &'static str,

    // ── 关于页签 ──
    pub about_tagline: &'static str,
    pub about_group_app: &'static str,
    pub about_group_runtime: &'static str,
    pub about_group_links: &'static str,
    pub about_group_thanks: &'static str,
    pub about_group_disclaimer: &'static str,
    pub about_version: &'static str,
    pub about_build: &'static str,
    pub about_stack: &'static str,
    pub about_license: &'static str,
    pub about_copyright: &'static str,
    pub about_os_arch: &'static str,
    pub about_config: &'static str,
    pub about_history: &'static str,
    pub about_thanks: &'static str,
    pub about_disclaimer: &'static str,
    pub about_open_link_tip: &'static str,

    // ── 另存对话框过滤器 ──
    pub filter_binary: &'static str,
    pub filter_all: &'static str,

    // ── fmt 层：解码输出内嵌文案 ──
    pub empty: &'static str,
    pub padded: &'static str,
    /// uuid 模式输入非 16 字节时的标注
    pub uuid_bad_len: &'static str,
    pub guess_empty_label: &'static str,
    pub guess_empty_sym: &'static str,
    pub guess_utf8: &'static str,
    pub guess_utf16_le: &'static str,
    pub guess_utf16_be: &'static str,
    pub guess_binary: &'static str,

    // ── 跳转/搜索输入解析错误 ──
    pub int_key_hint: &'static str,
    pub hex_even: &'static str,

    // ── db 层：表信息/错误 ──
    pub main_table: &'static str,
    pub flag_dup_sort: &'static str,
    pub flag_integer_key: &'static str,
    pub flag_dup_fixed: &'static str,
    pub flag_integer_dup: &'static str,
    pub flag_reverse_key: &'static str,
    pub flag_reverse_dup: &'static str,
    /// {p}
    pub path_missing_t: &'static str,
    /// {e}
    pub open_env_fail_t: &'static str,

    // ── 统计页 ──
    pub refresh: &'static str,
    pub g_geometry: &'static str,
    pub g_map: &'static str,
    pub g_txn: &'static str,
    pub g_readers: &'static str,
    pub g_main: &'static str,
    pub k_entries: &'static str,
    pub k_depth: &'static str,
    pub k_branch_pages: &'static str,
    pub k_leaf_pages: &'static str,
    pub k_overflow_pages: &'static str,
    pub k_page_size: &'static str,
    pub k_total_size: &'static str,
    pub k_table_flags: &'static str,
    pub none: &'static str,
    pub k_raw_flags: &'static str,
    pub k_min_size: &'static str,
    pub k_max_size: &'static str,
    pub k_current_size: &'static str,
    pub k_growth_step: &'static str,
    pub k_shrink_threshold: &'static str,
    pub k_map_size: &'static str,
    pub k_pages_used: &'static str,
    pub k_free_pages: &'static str,
    pub k_last_txnid: &'static str,
    pub k_max_readers: &'static str,
    pub k_num_readers: &'static str,
    pub k_branch: &'static str,
    pub k_leaf: &'static str,
    pub k_overflow: &'static str,
}

#[allow(clippy::too_many_arguments)]
impl I18n {
    // ── 带变量的句子（模板方法，允许各语言调整语序）──────────────

    /// 打开成功状态栏消息。
    pub fn open_ok(&self, mode: &str, n: usize, path: &str) -> String {
        fill(
            self.open_ok_t,
            &[("mode", mode), ("n", &n.to_string()), ("path", path)],
        )
    }

    /// 打开成功但没有任何表。
    pub fn open_no_tables(&self, path: &str) -> String {
        fill(self.open_no_tables_t, &[("path", path)])
    }

    pub fn read_fail(&self, e: &str) -> String {
        fill(self.read_fail_t, &[("e", e)])
    }

    pub fn jump_bad(&self, e: &str) -> String {
        fill(self.jump_bad_t, &[("e", e)])
    }

    pub fn jump_fail(&self, e: &str) -> String {
        fill(self.jump_fail_t, &[("e", e)])
    }

    pub fn export_ok(&self, n: usize, path: &str) -> String {
        fill(self.export_ok_t, &[("n", &n.to_string()), ("path", path)])
    }

    pub fn export_fail(&self, e: &str) -> String {
        fill(self.export_fail_t, &[("e", e)])
    }

    pub fn stat_fail(&self, e: &str) -> String {
        fill(self.stat_fail_t, &[("e", e)])
    }

    pub fn env_fail(&self, e: &str) -> String {
        fill(self.env_fail_t, &[("e", e)])
    }

    /// 左栏一行：显示名 + 条数 + 标志描述。
    pub fn table_entry(&self, name: &str, n: usize, flags_desc: &str) -> String {
        let flag = if flags_desc.is_empty() {
            String::new()
        } else {
            format!("{}{flags_desc}", self.table_flag_sep)
        };
        fill(
            self.table_entry_t,
            &[("name", name), ("n", &n.to_string()), ("flag", &flag)],
        )
    }

    /// 多值分组行 Value 列后缀，如 "〔5 个值〕"。
    pub fn dup_n_values(&self, n: usize) -> String {
        fill(self.dup_n_values_t, &[("n", &n.to_string())])
    }

    /// 大字段分段导航行。
    pub fn seg_line(&self, a: usize, b: usize, off: usize, total: usize) -> String {
        fill(
            self.seg_line_t,
            &[
                ("a", &a.to_string()),
                ("b", &b.to_string()),
                ("off", &off.to_string()),
                ("total", &total.to_string()),
                ("hex", &format!("{off:X}")),
            ],
        )
    }

    /// 多值表 Value 卡片标题，如 "Value（第 3/10 个值）"。
    pub fn val_title(&self, i: usize, n: usize) -> String {
        fill(
            self.val_title_t,
            &[("i", &i.to_string()), ("n", &n.to_string())],
        )
    }

    /// 自动猜测行，如 "猜测：UTF-8 文本，5 字节"。
    pub fn guess_line(&self, label: &str, n: usize) -> String {
        fill(
            self.guess_line_t,
            &[("label", label), ("n", &n.to_string())],
        )
    }

    /// 字节数，如 "5 字节"。
    pub fn bytes(&self, n: usize) -> String {
        fill(self.bytes_t, &[("n", &n.to_string())])
    }

    pub fn dup_located(&self, i: usize, total: usize) -> String {
        fill(
            self.dup_located_t,
            &[("i", &i.to_string()), ("total", &total.to_string())],
        )
    }

    pub fn searching_value_progress(&self, n: usize) -> String {
        fill(self.searching_value_progress_t, &[("n", &n.to_string())])
    }

    pub fn dup_wrap(&self, i: usize, total: usize) -> String {
        fill(
            self.dup_wrap_t,
            &[("i", &i.to_string()), ("total", &total.to_string())],
        )
    }

    pub fn dup_range(&self, n: usize, total: usize) -> String {
        fill(
            self.dup_range_t,
            &[("n", &n.to_string()), ("total", &total.to_string())],
        )
    }

    pub fn dup_bad_query(&self, e: &str) -> String {
        fill(self.dup_bad_query_t, &[("e", e)])
    }

    pub fn dup_search_fail(&self, e: &str) -> String {
        fill(self.dup_fail_t, &[("e", e)])
    }

    pub fn dup_load_fail(&self, e: &str) -> String {
        fill(self.dup_load_fail_t, &[("e", e)])
    }

    pub fn seg_ok(&self, off: usize) -> String {
        fill(
            self.seg_ok_t,
            &[("off", &off.to_string()), ("hex", &format!("{off:X}"))],
        )
    }

    pub fn seg_range_msg(&self, v: usize, total: usize) -> String {
        fill(
            self.seg_range_t,
            &[("v", &v.to_string()), ("total", &total.to_string())],
        )
    }

    pub fn blob_located(&self, off: usize) -> String {
        fill(self.blob_located_t, &[("hex", &format!("{off:X}"))])
    }

    pub fn blob_wrap(&self, off: usize) -> String {
        fill(self.blob_wrap_t, &[("hex", &format!("{off:X}"))])
    }

    pub fn page_keys(&self, n: usize) -> String {
        fill(self.page_keys_t, &[("n", &n.to_string())])
    }

    pub fn page_rows(&self, n: usize) -> String {
        fill(self.page_rows_t, &[("n", &n.to_string())])
    }

    pub fn total_pairs(&self, n: usize) -> String {
        fill(self.total_pairs_t, &[("n", &n.to_string())])
    }

    pub fn total_rows(&self, n: usize) -> String {
        fill(self.total_rows_t, &[("n", &n.to_string())])
    }

    pub fn range_keys(&self, range: &str) -> String {
        fill(self.range_keys_t, &[("range", range)])
    }

    pub fn range_rows(&self, range: &str) -> String {
        fill(self.range_rows_t, &[("range", range)])
    }

    pub fn window_size(&self, n: usize) -> String {
        fill(self.window_size_t, &[("n", &n.to_string())])
    }

    pub fn path_missing(&self, p: &str) -> String {
        fill(self.path_missing_t, &[("p", p)])
    }

    pub fn open_env_fail(&self, e: &str) -> String {
        fill(self.open_env_fail_t, &[("e", e)])
    }

    pub fn key_search_bad(&self, e: &str) -> String {
        fill(self.key_search_bad_t, &[("e", e)])
    }

    /// 导出进行中状态。
    pub fn export_progress(&self, n: usize) -> String {
        fill(self.export_progress_t, &[("n", &n.to_string())])
    }

    /// 导出完成状态（n 条记录到 path）。
    pub fn export_done(&self, n: usize, path: &str) -> String {
        fill(self.export_done_t, &[("n", &n.to_string()), ("path", path)])
    }
}

/// 中文（基准语言）。
const ZH: I18n = I18n {
    ready: "就绪",
    closed: "已关闭",

    btn_file: "文件",
    btn_dir: "目录",
    btn_close: "关闭",
    btn_reload_tip: "重新加载库：刷新表列表与条目数（保留当前表）",
    history: "历史",
    history_empty: "（暂无历史记录）",
    history_del_tip: "从历史记录中删除该条",
    endian: "字节序",
    endian_le: "小端 LE",
    endian_be: "大端 BE",
    cell_max_tip: "单元格最多显示的字符数（超出截断）",
    thousands_tip: "整数千位分隔（仅显示，不影响数据）",
    panel_tables: "表",
    panel_detail: "详情",

    mode_file: "单文件模式",
    mode_dir: "目录模式",

    mode_auto: "自动",

    tables_title: "表 (subDB)",
    filter_hint: "过滤表名",
    sort_name_asc: "名称 ⬆",
    sort_name_desc: "名称 ⬇",
    sort_count_asc: "条数 ⬆",
    sort_count_desc: "条数 ⬇",
    table_entry_t: "{name}  ({n} 条{flag})",
    table_flag_sep: "，",

    tab_data: "数据",
    tab_stat: "表统计",
    tab_env: "环境信息",
    tab_about: "关于",
    subtitle: "libmdbx 数据库查看工具（只读）",
    select_table: "请选择左侧表",

    default_order: "默认顺序",
    sort_hint: "点击列头排序：Key 列为全局遍历方向，其余列为当前页内排序",
    reset_order: "恢复为表中读取出来的顺序",
    tip_first: "首页（第一条）",
    tip_prev: "上一页",
    tip_next: "下一页",
    tip_last: "末页（最后一条）",
    page_size_label: "每页",
    page_size_tip: "每页显示条数",
    col_type: "类型",
    col_tip_key: "按 Key 排序 = 全表遍历方向（B+ 树索引顺序）\n点击切换升序 / 降序，翻页保持",
    col_tip_page: "页内排序：仅对当前已加载的这一页生效\n点击循环：升序 → 降序 → 默认顺序",
    dup_n_values_t: "〔{n} 个值〕",

    detail_select_hint: "在中间表格选择一行以查看详情",
    hex_addr_tip: "显示/隐藏地址列（文件内偏移）",
    addr: "地址",
    hex_col_tip: "显示/隐藏 HEX 字节列（HEX 与 ASCII 至少保留一项）",
    hex_ascii_tip: "显示/隐藏 ASCII 字符列（HEX 与 ASCII 至少保留一项）",
    hex_width_tip: "每行字节数：4/8/16，滚轮也可切换；右栏宽度随之自适应",
    copy: "复制",
    copied_row: "已复制选中行",
    save_as: "另存…",
    save_tip: "把完整原始字节保存为文件（不做任何截断）",
    seg_prev_tip: "上一段（64 KiB）",
    seg_next_tip: "下一段（64 KiB）",
    seg_input_hint: "偏移/0x..",
    seg_line_t: "第 {a}/{b} 段 · 偏移 {off} / {total}（0x{hex}）",
    val_title_t: "Value（第 {i}/{n} 个值）",
    dup_tip_first: "第一个值",
    dup_tip_prev100: "向前翻 100 个值",
    dup_tip_prev: "上一个值",
    dup_tip_next: "下一个值",
    dup_tip_next100: "向后翻 100 个值",
    dup_tip_last: "最后一个值",
    goto: "跳至",
    search_hint: "搜索值：文本或 hex(...)",
    search_prev_tip: "向前查找（值子串；到头回绕）",
    search_next_tip: "向后查找（回车等效；到头回绕）",
    guess_line_t: "猜测：{label}，{n} 字节",
    bytes_t: "{n} 字节",
    section_text: "文本",
    section_hex: "十六进制",
    hex_hint: "拖拽选择字节（HEX 与 ASCII 联动高亮），拖到上下边缘自动滚动可跨出可见区；Ctrl+C 同时复制十六进制与 ASCII 原文（按显示行对齐），Esc 或右键单击清除选区",

    dup_located_t: "已定位到第 {i}/{total} 个值",
    dup_wrap_t: "已回绕定位到第 {i}/{total} 个值",
    dup_range_t: "序号超出范围：{n}（共 {total} 个值）",
    dup_bad_num: "请输入有效的值序号（1 起的十进制数字）",
    dup_prompt: "请输入要搜索的值内容（文本或 hex(...)）",
    dup_bad_query_t: "搜索内容错误：{e}",
    dup_nomatch: "当前 Key 的值中没有匹配内容",
    dup_fail_t: "搜索失败：{e}",
    dup_load_fail_t: "加载值失败：{e}",
    seg_ok_t: "已跳至偏移 {off}（0x{hex}）",
    seg_range_t: "偏移超出范围：{v}（共 {total} 字节）",
    seg_bad: "请输入十进制偏移，或 0x 开头的十六进制偏移",
    blob_prompt: "请先输入要查找的内容（文本或 hex(...)）",
    blob_nomatch: "未找到匹配内容",
    blob_located_t: "已定位到偏移 0x{hex}",
    blob_wrap_t: "已回绕定位到偏移 0x{hex}",

    toolbar_search_hint: "搜索：文本或 hex(...)",
    key_mode_tip_off: "切换到过滤模式：K 键只显示以输入开头的 Key（回车生效）",
    key_mode_tip_on: "切换到跳转模式：K 键定位到首个不小于输入的 Key",
    key_search_btn_tip: "搜索 Key（跳转或前缀过滤）",
    value_search_tip_off: "搜索 Value：页内过滤（当前页文本包含，大小写不敏感）",
    value_search_tip_on: "搜索 Value：全表扫描（找到首个包含搜索词的记录并定位）",
    full_search_toggle: "全表",
    full_search_tip_off: "切换到全表搜索模式：V 键逐批扫描全表并定位到首个匹配",
    full_search_tip_on: "切换到页内过滤模式：V 键仅过滤当前已加载的行",
    search_empty: "请先输入搜索内容",
    filter_active: "已过滤",
    filter_clear_tip: "清除搜索过滤（Key 前缀 + Value 文本），恢复显示全表",
    key_search_bad_t: "搜索输入错误：{e}",

    export_tip: "把当前表导出为 CSV/JSON（后台线程，界面可继续操作）",
    export_started: "导出已开始…",
    export_progress_t: "导出中：已写出 {n} 条…",
    export_done_t: "导出完成：{n} 条，已保存到 {path}",

    theme_to_light: "切换到浅色主题",
    theme_to_dark: "切换到深色主题",
    favorites_title: "收藏",
    fav_add_t_tip: "收藏该表",
    fav_rm_t_tip: "取消收藏该表",
    fav_add_k_tip: "收藏该 Key",
    fav_rm_k_tip: "取消收藏该 Key",
    fav_jump_tip: "跳转到该 Key",
    fav_empty: "（暂无收藏）",

    open_ok_t: "已打开（{mode}模式，{n} 个表）：{path}",
    open_no_tables_t: "已打开但没有任何数据表：{path}",
    read_fail_t: "读取失败：{e}",
    jump_bad_t: "跳转输入错误：{e}",
    located: "已定位",
    not_found_ge: "未找到不小于该 key 的记录",
    searching_value: "正在全表搜索 Value…",
    searching_value_progress_t: "已扫描 {n} 条…",
    value_found: "已找到匹配项",
    value_not_found: "未找到匹配项",
    jump_fail_t: "跳转失败：{e}",
    export_ok_t: "已导出 {n} 字节到 {path}",
    export_fail_t: "导出失败：{e}",
    stat_fail_t: "读取表统计失败：{e}",
    env_fail_t: "读取环境信息失败：{e}",

    page_keys_t: "本页 {n} 个 Key",
    page_rows_t: "本页 {n} 条",
    total_pairs_t: "共 {n} 个值对",
    total_rows_t: "共 {n} 条",
    range_keys_t: "第 {range} 个 Key",
    range_rows_t: "第 {range} 条",
    window_size_t: "窗口 {n} 条",
    no_db_status: "未打开数据库 — 点“文件/目录”按钮，或将文件/目录拖入窗口",
    no_db_center: "请在上方打开数据库，或将文件/目录拖入窗口",

    about_tagline: "libmdbx 只读查看器",
    about_group_app: "应用",
    about_group_runtime: "运行环境",
    about_group_links: "链接",
    about_group_thanks: "致谢",
    about_group_disclaimer: "免责声明",
    about_version: "版本",
    about_build: "构建日期",
    about_stack: "技术栈",
    about_license: "许可证",
    about_copyright: "版权",
    about_os_arch: "系统 / 架构",
    about_config: "配置文件",
    about_history: "历史记录文件",
    about_thanks: "特别致谢 Trae（AI 编程助手）——本工具的全部代码、测试与文档均由 Trae 协助完成，功劳归于 Trae。同时致敬 libmdbx 作者 Leonid Yuriev、libmdbx-rs（vorot93 的 Rust 绑定），以及 egui / eframe 项目。",
    about_disclaimer: "本软件按“现状”提供，不附带任何明示或暗示的担保。虽然它仅以只读方式打开 MDBX 数据库，作者仍不对因使用本软件导致的任何数据损坏或损失承担责任。打开重要数据库前请自行做好备份。",
    about_open_link_tip: "在浏览器中打开",

    filter_binary: "二进制",
    filter_all: "所有文件",

    empty: "（空）",
    padded: "（补零）",
    uuid_bad_len: "（非 16 字节）",
    guess_empty_label: "空",
    guess_empty_sym: "空",
    guess_utf8: "UTF-8 文本",
    guess_utf16_le: "UTF-16 文本 (LE)",
    guess_utf16_be: "UTF-16 文本 (BE)",
    guess_binary: "二进制",

    int_key_hint: "整数键表请输入十进制数字，或 hex(...)/0x... 形式的字节",
    hex_even: "hex 长度必须为偶数",

    main_table: "（主表）",
    flag_dup_sort: "多值",
    flag_integer_key: "整数键",
    flag_dup_fixed: "定长多值",
    flag_integer_dup: "整数值",
    flag_reverse_key: "反序键",
    flag_reverse_dup: "反序值",
    path_missing_t: "路径不存在：{p}",
    open_env_fail_t: "打开环境失败：{e}",

    refresh: "刷新",
    g_geometry: "几何",
    g_map: "映射",
    g_txn: "事务",
    g_readers: "读者",
    g_main: "主表",
    k_entries: "条目数",
    k_depth: "B+树深度",
    k_branch_pages: "分支页数",
    k_leaf_pages: "叶子页数",
    k_overflow_pages: "溢出页数",
    k_page_size: "页大小",
    k_total_size: "数据总大小",
    k_table_flags: "表标志",
    none: "（无）",
    k_raw_flags: "标志位原始值",
    k_min_size: "文件下限",
    k_max_size: "文件上限",
    k_current_size: "当前大小",
    k_growth_step: "增长步长",
    k_shrink_threshold: "收缩阈值",
    k_map_size: "映射大小",
    k_pages_used: "已用页数",
    k_free_pages: "空闲页数",
    k_last_txnid: "最后事务 ID",
    k_max_readers: "读者槽位上限",
    k_num_readers: "当前读者数",
    k_branch: "分支页",
    k_leaf: "叶子页",
    k_overflow: "溢出页",
};

/// English.
const EN: I18n = I18n {
    ready: "Ready",
    closed: "Closed",

    btn_file: "File",
    btn_dir: "Folder",
    btn_close: "Close",
    btn_reload_tip: "Reload database: refresh table list and row counts (keep current table)",
    history: "History",
    history_empty: "(no history)",
    history_del_tip: "Remove this entry from history",
    endian: "Endian",
    endian_le: "Little LE",
    endian_be: "Big BE",
    cell_max_tip: "Max characters per cell (longer values are truncated)",
    thousands_tip: "Thousands separators (display only, data unchanged)",
    panel_tables: "Tables",
    panel_detail: "Detail",

    mode_file: "File mode",
    mode_dir: "Folder mode",

    mode_auto: "Auto",

    tables_title: "Tables (subDB)",
    filter_hint: "Filter tables",
    sort_name_asc: "Name ⬆",
    sort_name_desc: "Name ⬇",
    sort_count_asc: "Entries ⬆",
    sort_count_desc: "Entries ⬇",
    table_entry_t: "{name}  ({n}{flag})",
    table_flag_sep: ", ",

    tab_data: "Data",
    tab_stat: "Table stats",
    tab_env: "Environment",
    tab_about: "About",
    subtitle: "libmdbx database viewer (read-only)",
    select_table: "Select a table on the left",

    default_order: "Default order",
    sort_hint: "Click a header to sort: Key toggles global traversal direction; other columns sort the current page",
    reset_order: "Restore the order read from the table",
    tip_first: "First page",
    tip_prev: "Previous page",
    tip_next: "Next page",
    tip_last: "Last page",
    page_size_label: "Page",
    page_size_tip: "Rows per page",
    col_type: "Type",
    col_tip_key: "Sort by Key = whole-table traversal order (B+tree index)\nClick to toggle ascending / descending; kept across pages",
    col_tip_page: "In-page sort: affects only the currently loaded page\nClick to cycle: ascending → descending → default order",
    dup_n_values_t: " ({n} values)",

    detail_select_hint: "Select a row in the table to view details",
    hex_addr_tip: "Show/hide the address (in-file offset) column",
    addr: "Addr",
    hex_col_tip: "Show/hide the HEX byte column (at least one of HEX/ASCII stays)",
    hex_ascii_tip: "Show/hide the ASCII column (at least one of HEX/ASCII stays)",
    hex_width_tip: "Bytes per row: 4/8/16, mouse wheel also works; the panel width adjusts automatically",
    copy: "Copy",
    copied_row: "Row copied",
    save_as: "Save as…",
    save_tip: "Save the complete raw bytes to a file (no truncation)",
    seg_prev_tip: "Previous segment (64 KiB)",
    seg_next_tip: "Next segment (64 KiB)",
    seg_input_hint: "offset/0x..",
    seg_line_t: "Seg {a}/{b} · offset {off} / {total} (0x{hex})",
    val_title_t: "Value ({i}/{n})",
    dup_tip_first: "First value",
    dup_tip_prev100: "Back 100 values",
    dup_tip_prev: "Previous value",
    dup_tip_next: "Next value",
    dup_tip_next100: "Forward 100 values",
    dup_tip_last: "Last value",
    goto: "Go to",
    search_hint: "Search values: text or hex(...)",
    search_prev_tip: "Search backward (substring; wraps at ends)",
    search_next_tip: "Search forward (Enter; wraps at ends)",
    guess_line_t: "Guessed: {label}, {n} bytes",
    bytes_t: "{n} bytes",
    section_text: "Text",
    section_hex: "Hex",
    hex_hint: "Drag to select bytes (HEX and ASCII highlight together); hold near the top/bottom edge to auto-scroll beyond the visible area; Ctrl+C copies both hex and the ASCII text aligned by display row; Esc or right-click clears the selection",

    dup_located_t: "At value {i}/{total}",
    dup_wrap_t: "Wrapped to value {i}/{total}",
    dup_range_t: "Index out of range: {n} (total {total})",
    dup_bad_num: "Enter a valid value index (decimal, starting at 1)",
    dup_prompt: "Enter text or hex(...) to search for",
    dup_bad_query_t: "Invalid search input: {e}",
    dup_nomatch: "No matching value under this Key",
    dup_fail_t: "Search failed: {e}",
    dup_load_fail_t: "Failed to load values: {e}",
    seg_ok_t: "Jumped to offset {off} (0x{hex})",
    seg_range_t: "Offset out of range: {v} ({total} bytes)",
    seg_bad: "Enter a decimal offset, or hexadecimal prefixed with 0x",
    blob_prompt: "Enter text or hex(...) to search for",
    blob_nomatch: "No match found",
    blob_located_t: "At offset 0x{hex}",
    blob_wrap_t: "Wrapped to offset 0x{hex}",

    toolbar_search_hint: "Search: text or hex(...)",
    key_mode_tip_off: "Switch to filter mode: K shows only keys starting with the input (Enter to apply)",
    key_mode_tip_on: "Switch to jump mode: K locates the first key >= input",
    key_search_btn_tip: "Search keys (jump or prefix filter)",
    value_search_tip_off: "Search value: filter current page (case-insensitive contains)",
    value_search_tip_on: "Search value: full-table scan (locate the first record containing the term)",
    full_search_toggle: "Full",
    full_search_tip_off: "Switch to full-table search: V scans the whole table and locates the first match",
    full_search_tip_on: "Switch to page filter: V filters the currently loaded rows only",
    search_empty: "Enter a search term first",
    filter_active: "filtered",
    filter_clear_tip: "Clear search filters (key prefix + value text), show the whole table",
    key_search_bad_t: "Invalid search input: {e}",

    export_tip: "Export the current table as CSV/JSON (background thread; UI stays responsive)",
    export_started: "Export started…",
    export_progress_t: "Exporting: {n} records written…",
    export_done_t: "Export finished: {n} records saved to {path}",

    theme_to_light: "Switch to light theme",
    theme_to_dark: "Switch to dark theme",
    favorites_title: "Favorites",
    fav_add_t_tip: "Add this table to favorites",
    fav_rm_t_tip: "Remove this table from favorites",
    fav_add_k_tip: "Add this key to favorites",
    fav_rm_k_tip: "Remove this key from favorites",
    fav_jump_tip: "Jump to this key",
    fav_empty: "(no favorites yet)",

    open_ok_t: "Opened ({mode} mode, {n} tables): {path}",
    open_no_tables_t: "Opened, but it contains no data tables: {path}",
    read_fail_t: "Read failed: {e}",
    jump_bad_t: "Invalid jump input: {e}",
    located: "Located",
    not_found_ge: "No record found with a key greater than or equal to this one",
    searching_value: "Searching all values…",
    searching_value_progress_t: "Scanned {n} entries…",
    value_found: "Match found",
    value_not_found: "No match found",
    jump_fail_t: "Jump failed: {e}",
    export_ok_t: "Exported {n} bytes to {path}",
    export_fail_t: "Export failed: {e}",
    stat_fail_t: "Failed to read table stats: {e}",
    env_fail_t: "Failed to read environment info: {e}",

    page_keys_t: "Page: {n} keys",
    page_rows_t: "Page: {n} rows",
    total_pairs_t: "{n} key-value pairs total",
    total_rows_t: "{n} rows total",
    range_keys_t: "Keys {range}",
    range_rows_t: "Rows {range}",
    window_size_t: "Page size {n}",
    no_db_status: "No database open — use File/Folder, or drop a file/folder into the window",
    no_db_center: "Open a database above, or drop a file/folder into the window",

    about_tagline: "A read-only viewer for libmdbx databases",
    about_group_app: "Application",
    about_group_runtime: "Runtime",
    about_group_links: "Links",
    about_group_thanks: "Acknowledgments",
    about_group_disclaimer: "Disclaimer",
    about_version: "Version",
    about_build: "Build date",
    about_stack: "Built with",
    about_license: "License",
    about_copyright: "Copyright",
    about_os_arch: "OS / Architecture",
    about_config: "Config file",
    about_history: "History file",
    about_thanks: "Special thanks to Trae (AI coding assistant) — every line of code, test and documentation in this tool was created with Trae; all the credit goes to Trae. Also tribute to Leonid Yuriev for libmdbx, to libmdbx-rs (the Rust binding by vorot93), and to the egui / eframe project.",
    about_disclaimer: "This software is provided \"AS IS\" without any express or implied warranty. Although it opens MDBX databases read-only, the author is not liable for any data corruption or loss caused by using this software. Back up important databases before opening them.",
    about_open_link_tip: "Open in browser",

    filter_binary: "Binary",
    filter_all: "All files",

    empty: "(empty)",
    padded: "(zero-padded)",
    uuid_bad_len: "(not 16 bytes)",
    guess_empty_label: "Empty",
    guess_empty_sym: "empty",
    guess_utf8: "UTF-8 text",
    guess_utf16_le: "UTF-16 text (LE)",
    guess_utf16_be: "UTF-16 text (BE)",
    guess_binary: "Binary",

    int_key_hint: "For integer-key tables enter a decimal number, or bytes as hex(...)/0x...",
    hex_even: "hex length must be even",

    main_table: "(main)",
    flag_dup_sort: "dup-sort",
    flag_integer_key: "integer key",
    flag_dup_fixed: "fixed dup",
    flag_integer_dup: "integer dup",
    flag_reverse_key: "reverse key",
    flag_reverse_dup: "reverse dup",
    path_missing_t: "Path not found: {p}",
    open_env_fail_t: "Failed to open environment: {e}",

    refresh: "Refresh",
    g_geometry: "Geometry",
    g_map: "Memory map",
    g_txn: "Transaction",
    g_readers: "Readers",
    g_main: "Main table",
    k_entries: "Entries",
    k_depth: "B+tree depth",
    k_branch_pages: "Branch pages",
    k_leaf_pages: "Leaf pages",
    k_overflow_pages: "Overflow pages",
    k_page_size: "Page size",
    k_total_size: "Total data size",
    k_table_flags: "Table flags",
    none: "(none)",
    k_raw_flags: "Raw flags",
    k_min_size: "Min size",
    k_max_size: "Max size",
    k_current_size: "Current size",
    k_growth_step: "Growth step",
    k_shrink_threshold: "Shrink threshold",
    k_map_size: "Map size",
    k_pages_used: "Pages used",
    k_free_pages: "Free pages",
    k_last_txnid: "Last txn ID",
    k_max_readers: "Max readers",
    k_num_readers: "Active readers",
    k_branch: "Branch pages",
    k_leaf: "Leaf pages",
    k_overflow: "Overflow pages",
};

/// Русский.
const RU: I18n = I18n {
    ready: "Готово",
    closed: "Закрыто",

    btn_file: "Файл",
    btn_dir: "Папка",
    btn_close: "Закрыть",
    btn_reload_tip: "Перезагрузить базу: обновить список таблиц и число записей (текущая таблица сохраняется)",
    history: "История",
    history_empty: "(история пуста)",
    history_del_tip: "Удалить эту запись из истории",
    endian: "Порядок байт",
    endian_le: "Младший LE",
    endian_be: "Старший BE",
    cell_max_tip: "Макс. символов в ячейке (длинные значения обрезаются)",
    thousands_tip: "Разделители тысяч (только отображение, данные не меняются)",
    panel_tables: "Таблицы",
    panel_detail: "Детали",

    mode_file: "Файловый режим",
    mode_dir: "Режим каталога",

    mode_auto: "Авто",

    tables_title: "Таблицы (subDB)",
    filter_hint: "Фильтр таблиц",
    sort_name_asc: "Имя ⬆",
    sort_name_desc: "Имя ⬇",
    sort_count_asc: "Записи ⬆",
    sort_count_desc: "Записи ⬇",
    table_entry_t: "{name}  ({n}{flag})",
    table_flag_sep: ", ",

    tab_data: "Данные",
    tab_stat: "Статистика",
    tab_env: "Окружение",
    tab_about: "О программе",
    subtitle: "Просмотр баз данных libmdbx (только чтение)",
    select_table: "Выберите таблицу слева",

    default_order: "Исходный порядок",
    sort_hint: "Щелчок по заголовку: Key меняет глобальное направление обхода; остальные колонки сортируют текущую страницу",
    reset_order: "Восстановить порядок чтения из таблицы",
    tip_first: "Первая страница",
    tip_prev: "Предыдущая страница",
    tip_next: "Следующая страница",
    tip_last: "Последняя страница",
    page_size_label: "Стр.",
    page_size_tip: "Строк на странице",
    col_type: "Тип",
    col_tip_key: "Сортировка по Key = порядок обхода всей таблицы (индекс B+дерева)\nЩелчок: по возрастанию / по убыванию, сохраняется при листании",
    col_tip_page: "Сортировка в пределах страницы: только текущая загруженная страница\nЩелчок: по возрастанию → по убыванию → исходный порядок",
    dup_n_values_t: " ({n} знач.)",

    detail_select_hint: "Выберите строку в таблице для просмотра деталей",
    hex_addr_tip: "Показать/скрыть колонку адресов (смещение в файле)",
    addr: "Адр.",
    hex_col_tip: "Показать/скрыть колонку HEX-байтов (HEX и ASCII: хотя бы одна остаётся)",
    hex_ascii_tip: "Показать/скрыть колонку ASCII (HEX и ASCII: хотя бы одна остаётся)",
    hex_width_tip: "Байт в строке: 4/8/16, также колесо мыши; ширина панели подстраивается автоматически",
    copy: "Копия",
    copied_row: "Строка скопирована",
    save_as: "Сохранить…",
    save_tip: "Сохранить все исходные байты в файл (без обрезки)",
    seg_prev_tip: "Предыдущий сегмент (64 КиБ)",
    seg_next_tip: "Следующий сегмент (64 КиБ)",
    seg_input_hint: "смещение/0x..",
    seg_line_t: "Сегм {a}/{b} · смещение {off} / {total} (0x{hex})",
    val_title_t: "Значение ({i}/{n})",
    dup_tip_first: "Первое значение",
    dup_tip_prev100: "Назад на 100 значений",
    dup_tip_prev: "Предыдущее значение",
    dup_tip_next: "Следующее значение",
    dup_tip_next100: "Вперёд на 100 значений",
    dup_tip_last: "Последнее значение",
    goto: "К №",
    search_hint: "Поиск значений: текст или hex(...)",
    search_prev_tip: "Искать назад (подстрока; с переходом в конце)",
    search_next_tip: "Искать вперёд (Enter; с переходом в конце)",
    guess_line_t: "Тип: {label}, {n} байт",
    bytes_t: "{n} байт",
    section_text: "Текст",
    section_hex: "Hex",
    hex_hint: "Перетащите для выбора байт (HEX и ASCII подсвечиваются вместе); удерживайте у верхнего/нижнего края для автопрокрутки за пределы видимой области; Ctrl+C копирует и hex, и исходный текст ASCII с выравниванием по строкам; Esc или правая кнопка мыши снимают выделение",

    dup_located_t: "Значение {i}/{total}",
    dup_wrap_t: "С переходом к значению {i}/{total}",
    dup_range_t: "Индекс вне диапазона: {n} (всего {total})",
    dup_bad_num: "Введите допустимый номер значения (целое, начиная с 1)",
    dup_prompt: "Введите текст или hex(...) для поиска",
    dup_bad_query_t: "Неверный запрос поиска: {e}",
    dup_nomatch: "Среди значений этого Key совпадений нет",
    dup_fail_t: "Ошибка поиска: {e}",
    dup_load_fail_t: "Не удалось загрузить значения: {e}",
    seg_ok_t: "Переход к смещению {off} (0x{hex})",
    seg_range_t: "Смещение вне диапазона: {v} ({total} байт)",
    seg_bad: "Введите десятичное смещение или шестнадцатеричное с префиксом 0x",
    blob_prompt: "Введите текст или hex(...) для поиска",
    blob_nomatch: "Совпадений не найдено",
    blob_located_t: "Смещение 0x{hex}",
    blob_wrap_t: "С переходом к смещению 0x{hex}",

    toolbar_search_hint: "Поиск: текст или hex(...)",
    key_mode_tip_off: "Переключиться в режим фильтра: K показывает только ключи, начинающиеся с ввода (Enter — применить)",
    key_mode_tip_on: "Переключиться в режим перехода: K переходит к первому ключу >= ввода",
    key_search_btn_tip: "Поиск ключей (переход или фильтр по префиксу)",
    value_search_tip_off: "Поиск значения: фильтр по текущей странице (вхождение без учёта регистра)",
    value_search_tip_on: "Поиск значения: сканирование всей таблицы (переход к первой записи с совпадением)",
    full_search_toggle: "Вся",
    full_search_tip_off: "Переключиться на поиск по всей таблице: V сканирует её и переходит к первому совпадению",
    full_search_tip_on: "Переключиться на фильтр по странице: V фильтрует только загруженные строки",
    search_empty: "Сначала введите поисковый запрос",
    filter_active: "отфильтровано",
    filter_clear_tip: "Сбросить фильтры поиска (префикс ключа + текст значения), показать всю таблицу",
    key_search_bad_t: "Неверный запрос поиска: {e}",

    export_tip: "Экспорт текущей таблицы в CSV/JSON (в фоне; интерфейс остаётся отзывчивым)",
    export_started: "Экспорт начат…",
    export_progress_t: "Экспорт: записано {n} записей…",
    export_done_t: "Экспорт завершён: {n} записей сохранено в {path}",

    theme_to_light: "Переключиться на светлую тему",
    theme_to_dark: "Переключиться на тёмную тему",
    favorites_title: "Избранное",
    fav_add_t_tip: "Добавить таблицу в избранное",
    fav_rm_t_tip: "Убрать таблицу из избранного",
    fav_add_k_tip: "Добавить ключ в избранное",
    fav_rm_k_tip: "Убрать ключ из избранного",
    fav_jump_tip: "Перейти к этому ключу",
    fav_empty: "(избранное пусто)",

    open_ok_t: "Открыто (режим: {mode}, таблиц: {n}): {path}",
    open_no_tables_t: "Открыто, но таблиц данных нет: {path}",
    read_fail_t: "Ошибка чтения: {e}",
    jump_bad_t: "Неверный ввод перехода: {e}",
    located: "Найдено",
    not_found_ge: "Не найдено записи с ключом больше или равным заданному",
    searching_value: "Поиск по всем значениям…",
    searching_value_progress_t: "Проверено {n} записей…",
    value_found: "Совпадение найдено",
    value_not_found: "Совпадений не найдено",
    jump_fail_t: "Ошибка перехода: {e}",
    export_ok_t: "Экспортировано {n} байт в {path}",
    export_fail_t: "Ошибка экспорта: {e}",
    stat_fail_t: "Не удалось прочитать статистику таблицы: {e}",
    env_fail_t: "Не удалось прочитать информацию окружения: {e}",

    page_keys_t: "На странице {n} ключей",
    page_rows_t: "На странице {n} записей",
    total_pairs_t: "всего {n} пар",
    total_rows_t: "всего {n} записей",
    range_keys_t: "Ключи {range}",
    range_rows_t: "Записи {range}",
    window_size_t: "Размер страницы {n}",
    no_db_status: "База не открыта — нажмите «Файл/Папка» или перетащите файл/папку в окно",
    no_db_center: "Откройте базу выше или перетащите файл/папку в окно",

    about_tagline: "Просмотрщик баз libmdbx только для чтения",
    about_group_app: "Приложение",
    about_group_runtime: "Среда",
    about_group_links: "Ссылки",
    about_group_thanks: "Благодарности",
    about_group_disclaimer: "Отказ от ответственности",
    about_version: "Версия",
    about_build: "Дата сборки",
    about_stack: "Технологии",
    about_license: "Лицензия",
    about_copyright: "Авторское право",
    about_os_arch: "ОС / Архитектура",
    about_config: "Файл настроек",
    about_history: "Файл истории",
    about_thanks: "Особая благодарность Trae (ИИ-помощнику для программирования) — весь код, тесты и документация этого инструмента созданы при помощи Trae, вся заслуга принадлежит Trae. Также благодарим Леонида Юрьева за libmdbx, привязку libmdbx-rs (Rust-биндинг от vorot93) и проект egui / eframe.",
    about_disclaimer: "Программа поставляется «КА ЕСТЬ», без каких-либо явных или подразумеваемых гарантий. Хотя она открывает базы MDBX только для чтения, автор не несёт ответственности за любое повреждение или потерю данных при использовании программы. Перед открытием важных баз делайте резервные копии.",
    about_open_link_tip: "Открыть в браузере",

    filter_binary: "Бинарные",
    filter_all: "Все файлы",

    empty: "(пусто)",
    padded: "(доп. нулями)",
    uuid_bad_len: "(не 16 байт)",
    guess_empty_label: "Пусто",
    guess_empty_sym: "пусто",
    guess_utf8: "Текст UTF-8",
    guess_utf16_le: "Текст UTF-16 (LE)",
    guess_utf16_be: "Текст UTF-16 (BE)",
    guess_binary: "Бинарный",

    int_key_hint: "Для таблиц с целочисленными ключами введите десятичное число или байты в виде hex(...)/0x...",
    hex_even: "длина hex должна быть чётной",

    main_table: "(основная)",
    flag_dup_sort: "дубли",
    flag_integer_key: "цел. ключ",
    flag_dup_fixed: "фикс. дубли",
    flag_integer_dup: "цел. значения",
    flag_reverse_key: "обр. ключ",
    flag_reverse_dup: "обр. значения",
    path_missing_t: "Путь не найден: {p}",
    open_env_fail_t: "Не удалось открыть среду: {e}",

    refresh: "Обновить",
    g_geometry: "Геометрия",
    g_map: "Отображение",
    g_txn: "Транзакция",
    g_readers: "Читатели",
    g_main: "Основная таблица",
    k_entries: "Записей",
    k_depth: "Глубина B+дерева",
    k_branch_pages: "Страниц ветвей",
    k_leaf_pages: "Листовых страниц",
    k_overflow_pages: "Страниц переполнения",
    k_page_size: "Размер страницы",
    k_total_size: "Общий размер данных",
    k_table_flags: "Флаги таблицы",
    none: "(нет)",
    k_raw_flags: "Сырые флаги",
    k_min_size: "Мин. размер",
    k_max_size: "Макс. размер",
    k_current_size: "Текущий размер",
    k_growth_step: "Шаг роста",
    k_shrink_threshold: "Порог сжатия",
    k_map_size: "Размер отображения",
    k_pages_used: "Занято страниц",
    k_free_pages: "Свободных страниц",
    k_last_txnid: "Посл. ID транзакции",
    k_max_readers: "Макс. читателей",
    k_num_readers: "Читателей сейчас",
    k_branch: "Стр. ветвей",
    k_leaf: "Листовые стр.",
    k_overflow: "Стр. переполнения",
};

static TABLES: [I18n; 3] = [ZH, EN, RU];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_code_round_trip() {
        for l in Lang::ALL {
            assert_eq!(Lang::from_code(l.code()), l);
        }
    }

    #[test]
    fn lang_from_code_unknown_falls_back_to_zh() {
        assert_eq!(Lang::from_code("fr"), Lang::Zh);
        assert_eq!(Lang::from_code(""), Lang::Zh);
        assert_eq!(Lang::from_code("ZH"), Lang::Zh); // 大小写敏感
    }

    #[test]
    fn lang_label_is_self_naming() {
        assert_eq!(Lang::Zh.label(), "中文");
        assert_eq!(Lang::En.label(), "English");
        assert_eq!(Lang::Ru.label(), "Русский");
    }

    #[test]
    fn lang_discriminants_stable() {
        // 与 config 持久化的 u8 对应关系不能变
        assert_eq!(Lang::Zh as u8, 0);
        assert_eq!(Lang::En as u8, 1);
        assert_eq!(Lang::Ru as u8, 2);
    }

    #[test]
    fn set_and_get_lang() {
        let prev = lang();
        for l in Lang::ALL {
            set_lang(l);
            assert_eq!(lang(), l);
        }
        set_lang(prev);
    }

    #[test]
    fn tr_matches_selected_lang() {
        let prev = lang();
        set_lang(Lang::Zh);
        // 中文表里 btn_file 是"文件"
        assert_eq!(tr().btn_file, "文件");
        set_lang(Lang::En);
        assert_eq!(tr().btn_file, "File");
        set_lang(Lang::Ru);
        assert_eq!(tr().btn_file, "Файл");
        set_lang(prev);
    }

    #[test]
    fn fill_substitutes_placeholders() {
        let prev = lang();
        set_lang(Lang::En);
        let t = tr();
        // open_ok_t 模板含 {mode} {n} {path}
        let s = t.open_ok("dir", 5, "/tmp/db");
        assert!(s.contains("dir"), "got: {s}");
        assert!(s.contains("5"), "got: {s}");
        assert!(s.contains("/tmp/db"), "got: {s}");
        set_lang(prev);
    }

    #[test]
    fn all_tables_have_non_empty_core_fields() {
        // 三张表的核心字段都不应为空（防止翻译漏填）
        for table in &TABLES {
            assert!(!table.ready.is_empty());
            assert!(!table.btn_file.is_empty());
            assert!(!table.btn_close.is_empty());
            assert!(!table.btn_reload_tip.is_empty());
            assert!(!table.tables_title.is_empty());
            assert!(!table.detail_select_hint.is_empty());
            assert!(!table.dup_tip_first.is_empty());
        }
    }

    #[test]
    fn all_tables_have_key_search_fields() {
        for table in &TABLES {
            assert!(!table.toolbar_search_hint.is_empty());
            assert!(!table.key_search_btn_tip.is_empty());
            assert!(!table.value_search_tip_off.is_empty());
            assert!(!table.value_search_tip_on.is_empty());
            assert!(!table.full_search_toggle.is_empty());
            assert!(!table.full_search_tip_off.is_empty());
            assert!(!table.full_search_tip_on.is_empty());
            assert!(!table.key_mode_tip_off.is_empty());
            assert!(!table.key_mode_tip_on.is_empty());
            assert!(!table.filter_active.is_empty());
            assert!(!table.filter_clear_tip.is_empty());
            assert!(!table.key_search_bad_t.is_empty());
            assert!(table.key_search_bad_t.contains("{e}"));
            assert!(table.key_search_bad("oops").contains("oops"));
        }
    }

    #[test]
    fn all_tables_have_export_fields() {
        for table in &TABLES {
            assert!(!table.export_tip.is_empty());
            assert!(!table.export_started.is_empty());
            assert!(table.export_progress_t.contains("{n}"));
            assert!(table.export_done_t.contains("{n}"));
            assert!(table.export_done_t.contains("{path}"));
            assert!(table.export_progress(5).contains("5"));
            let s = table.export_done(7, "/tmp/a.csv");
            assert!(s.contains("7"), "got: {s}");
            assert!(s.contains("/tmp/a.csv"), "got: {s}");
        }
    }

    #[test]
    fn all_tables_have_theme_favorites_fields() {
        for table in &TABLES {
            assert!(!table.theme_to_light.is_empty());
            assert!(!table.theme_to_dark.is_empty());
            assert!(!table.favorites_title.is_empty());
            assert!(!table.fav_add_t_tip.is_empty());
            assert!(!table.fav_rm_t_tip.is_empty());
            assert!(!table.fav_add_k_tip.is_empty());
            assert!(!table.fav_rm_k_tip.is_empty());
            assert!(!table.fav_jump_tip.is_empty());
            assert!(!table.fav_empty.is_empty());
        }
    }

    #[test]
    fn all_tables_have_column_sort_tips() {
        for table in &TABLES {
            assert!(!table.col_tip_key.is_empty());
            assert!(!table.col_tip_page.is_empty());
        }
    }

    #[test]
    fn all_tables_have_about_fields() {
        for table in &TABLES {
            assert!(!table.tab_about.is_empty());
            assert!(!table.about_tagline.is_empty());
            assert!(!table.about_group_app.is_empty());
            assert!(!table.about_group_runtime.is_empty());
            assert!(!table.about_group_links.is_empty());
            assert!(!table.about_group_thanks.is_empty());
            assert!(!table.about_group_disclaimer.is_empty());
            assert!(!table.about_version.is_empty());
            assert!(!table.about_build.is_empty());
            assert!(!table.about_stack.is_empty());
            assert!(!table.about_license.is_empty());
            assert!(!table.about_copyright.is_empty());
            assert!(!table.about_os_arch.is_empty());
            assert!(!table.about_config.is_empty());
            assert!(!table.about_history.is_empty());
            assert!(!table.about_thanks.is_empty());
            assert!(!table.about_disclaimer.is_empty());
            assert!(!table.about_open_link_tip.is_empty());
        }
    }
}
