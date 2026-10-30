// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 表统计与环境信息，格式化为纯键值行供 UI 直接渲染。
//! 标签文案随当前界面语言。

use libmdbx::{Database, NoWriteMap};

/// 数值加千位分隔（与列表/解码显示共用同一全局开关）。
fn num(v: impl ToString) -> String {
    crate::fmt::with_sep(v.to_string())
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} B", num(bytes))
    } else {
        format!("{v:.2} {} ({} B)", UNITS[i], num(bytes))
    }
}

/// 表统计页签内容：(标签, 值)。
///
/// - `table`：None = 主表
pub fn table_stat_view(
    db: &Database<NoWriteMap>,
    table: Option<&str>,
    flags_desc: &str,
) -> Result<Vec<(String, String)>, String> {
    let t = crate::i18n::tr();
    let txn = db.begin_ro_txn().map_err(|e| e.to_string())?;
    let table = txn.open_table(table).map_err(|e| e.to_string())?;
    let stat = txn.table_stat(&table).map_err(|e| e.to_string())?;
    let flags = txn.table_flags(&table).map_err(|e| e.to_string())?;

    Ok(vec![
        (t.k_entries.to_string(), num(stat.entries())),
        (t.k_depth.to_string(), num(stat.depth())),
        (t.k_branch_pages.to_string(), num(stat.branch_pages())),
        (t.k_leaf_pages.to_string(), num(stat.leaf_pages())),
        (t.k_overflow_pages.to_string(), num(stat.overflow_pages())),
        (
            t.k_page_size.to_string(),
            human_size(stat.page_size() as u64),
        ),
        (t.k_total_size.to_string(), human_size(stat.total_size())),
        (
            t.k_table_flags.to_string(),
            if flags_desc.is_empty() {
                t.none.to_string()
            } else {
                flags_desc.to_string()
            },
        ),
        (t.k_raw_flags.to_string(), format!("0x{:X}", flags.bits())),
    ])
}

/// 环境信息页签内容：(分组, 标签, 值)。
pub fn env_info_view(db: &Database<NoWriteMap>) -> Result<Vec<(String, String, String)>, String> {
    let t = crate::i18n::tr();
    let info = db.info().map_err(|e| e.to_string())?;
    let stat = db.stat().map_err(|e| e.to_string())?;
    let freelist = db.freelist().unwrap_or(0);
    let geo = info.geometry();

    let mut rows = Vec::new();
    let mut push = |g: &str, k: &str, v: String| rows.push((g.to_string(), k.to_string(), v));

    push(t.g_geometry, t.k_min_size, human_size(geo.min_size()));
    push(t.g_geometry, t.k_max_size, human_size(geo.max_size()));
    push(
        t.g_geometry,
        t.k_current_size,
        human_size(geo.current_size()),
    );
    push(t.g_geometry, t.k_growth_step, human_size(geo.growth_step()));
    push(
        t.g_geometry,
        t.k_shrink_threshold,
        human_size(geo.shrink_threshold()),
    );
    push(t.g_map, t.k_map_size, human_size(info.map_size() as u64));
    push(t.g_map, t.k_pages_used, num(info.last_pgno() + 1));
    push(t.g_map, t.k_free_pages, num(freelist));
    push(t.g_txn, t.k_last_txnid, num(info.last_txnid()));
    push(t.g_readers, t.k_max_readers, num(info.max_readers()));
    push(t.g_readers, t.k_num_readers, num(info.num_readers()));
    push(t.g_main, t.k_page_size, human_size(stat.page_size() as u64));
    push(t.g_main, t.k_depth, num(stat.depth()));
    push(t.g_main, t.k_branch, num(stat.branch_pages()));
    push(t.g_main, t.k_leaf, num(stat.leaf_pages()));
    push(t.g_main, t.k_overflow, num(stat.overflow_pages()));
    push(t.g_main, t.k_entries, num(stat.entries()));

    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::{human_size, num};

    #[test]
    fn stats_numbers_use_thousands_sep() {
        let prev = crate::fmt::thousands_sep();
        crate::fmt::set_thousands_sep(true);
        assert_eq!(num(20000u64), "20,000");
        assert_eq!(human_size(1_474_560), "1.41 MB (1,474,560 B)");
        crate::fmt::set_thousands_sep(false);
        assert_eq!(num(20000u64), "20000");
        crate::fmt::set_thousands_sep(prev);
    }

    #[test]
    fn human_size_bytes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1023), "1023 B");
    }

    #[test]
    fn human_size_kb() {
        let s = human_size(1024);
        assert!(s.contains("KB"), "got: {s}");
        assert!(s.contains("1024 B")); // 附带原始字节
    }

    #[test]
    fn human_size_mb_gb() {
        let s = human_size(1024 * 1024 * 5);
        assert!(s.contains("MB"), "got: {s}");
        let s = human_size(1024u64 * 1024 * 1024 * 2);
        assert!(s.contains("GB"), "got: {s}");
    }

    #[test]
    fn human_size_tb_caps_at_tb() {
        let s = human_size(1024u64 * 1024 * 1024 * 1024 * 100);
        // 超过 TB 仍以 TB 表示（UNIT 数组到 TB 为止）
        assert!(s.contains("TB"), "got: {s}");
    }
}
