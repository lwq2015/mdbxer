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

    /// 排版下拉分组标题（DecodeMode::GROUPS，顺序一一对应）
    pub grp_text: &'static str,
    pub grp_num: &'static str,
    pub grp_bytes: &'static str,
    pub grp_ident: &'static str,
    pub grp_struct: &'static str,
    /// Protobuf：载入 .proto 按钮
    pub proto_load: &'static str,
    /// Protobuf：载入按钮 hover 提示
    pub proto_load_tip: &'static str,
    /// Protobuf：类型下拉未选时的占位文本
    pub proto_pick: &'static str,
    /// Protobuf：未载入 .proto 时类型下拉的占位项
    pub proto_no_types: &'static str,
    /// proto 关联表清除按钮
    pub proto_clear: &'static str,
    pub proto_clear_tip: &'static str,

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
    /// 类型列在 Value 工具栏指定了解码类型时的 hover 说明
    pub type_forced_tip: &'static str,
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
    /// 搜索框 hover 提示（快捷键说明）
    pub search_box_tip: &'static str,
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
    /// 收藏表行后缀：该表下收藏的 Key 数量，如 "（收藏 3 条）"
    pub fav_count_t: &'static str,

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
    /// 全表续扫到尽头后回绕扫描的进度 {n}
    pub searching_value_wrap_t: &'static str,
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
    /// mac/ip/time 手动格式长度不符提示
    pub mac_bad_len: &'static str,
    pub ip_bad_len: &'static str,
    pub time_bad_len: &'static str,
    pub guess_empty_label: &'static str,
    pub guess_empty_sym: &'static str,
    pub guess_utf8: &'static str,
    pub guess_utf16_le: &'static str,
    pub guess_utf16_be: &'static str,
    pub guess_binary: &'static str,

    // ── 高级配置页 ──
    /// 中央页签名
    pub tab_settings: &'static str,
    /// 附注开关表节标题与提示
    pub hint_section: &'static str,
    pub hint_section_tip: &'static str,
    /// 附注表列头「格式」与行名
    pub hint_col_format: &'static str,
    pub hint_ts: &'static str,
    pub hint_filetime: &'static str,
    pub hint_ipv4: &'static str,
    pub hint_mac: &'static str,
    pub hint_ipv6: &'static str,
    pub hint_magic: &'static str,
    /// 显示参数节标题与「千位分隔」标签
    pub disp_section: &'static str,
    pub thousands_label: &'static str,
    /// 「单元格显示长度」行标签（原顶栏无标签，高级配置页新增）
    pub cell_max_label: &'static str,

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
    // ── 只读打开失败 → 恢复确认 ──
    pub recover_title: &'static str,
    /// {e}
    pub recover_msg_t: &'static str,
    pub recover_btn: &'static str,
    pub recover_backup_btn: &'static str,
    pub cancel: &'static str,
    /// {e}
    pub backup_fail_t: &'static str,

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

    /// 收藏区表行后缀：该表下收藏的 Key 数量，如 "（收藏 3 条）"。
    pub fn fav_count(&self, n: usize) -> String {
        fill(self.fav_count_t, &[("n", &n.to_string())])
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

    pub fn searching_value_wrap(&self, n: usize) -> String {
        fill(self.searching_value_wrap_t, &[("n", &n.to_string())])
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

    /// 只读打开失败后的恢复确认说明。
    pub fn recover_msg(&self, e: &str) -> String {
        fill(self.recover_msg_t, &[("e", e)])
    }

    /// 恢复前备份失败。
    pub fn backup_fail(&self, e: &str) -> String {
        fill(self.backup_fail_t, &[("e", e)])
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

mod en;
mod ru;
mod zh;

use en::EN;
use ru::RU;
use zh::ZH;

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
            assert!(!table.search_box_tip.is_empty());
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
            assert!(!table.fav_count_t.is_empty());
        }
    }

    #[test]
    fn all_tables_have_column_sort_tips() {
        for table in &TABLES {
            assert!(!table.col_tip_key.is_empty());
            assert!(!table.col_tip_page.is_empty());
            assert!(!table.type_forced_tip.is_empty());
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
