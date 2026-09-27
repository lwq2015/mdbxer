//! 表统计与环境信息，格式化为纯键值行供 UI 直接渲染。

use libmdbx::{Database, NoWriteMap};

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.2} {} ({bytes} B)", UNITS[i])
    }
}

/// 表统计页签内容：(标签, 值)。
pub fn table_stat_view(
    db: &Database<NoWriteMap>,
    table: Option<&str>,
    flags_desc: &str,
) -> Result<Vec<(String, String)>, String> {
    let txn = db.begin_ro_txn().map_err(|e| e.to_string())?;
    let table = txn.open_table(table).map_err(|e| e.to_string())?;
    let stat = txn.table_stat(&table).map_err(|e| e.to_string())?;
    let flags = txn.table_flags(&table).map_err(|e| e.to_string())?;

    Ok(vec![
        ("条目数".into(), stat.entries().to_string()),
        ("B+树深度".into(), stat.depth().to_string()),
        ("分支页数".into(), stat.branch_pages().to_string()),
        ("叶子页数".into(), stat.leaf_pages().to_string()),
        ("溢出页数".into(), stat.overflow_pages().to_string()),
        ("页大小".into(), human_size(stat.page_size() as u64)),
        ("数据总大小".into(), human_size(stat.total_size())),
        (
            "表标志".into(),
            if flags_desc.is_empty() {
                "（无）".into()
            } else {
                flags_desc.to_string()
            },
        ),
        ("标志位原始值".into(), format!("0x{:X}", flags.bits())),
    ])
}

/// 环境信息页签内容：(分组, 标签, 值)。
pub fn env_info_view(db: &Database<NoWriteMap>) -> Result<Vec<(String, String, String)>, String> {
    let info = db.info().map_err(|e| e.to_string())?;
    let stat = db.stat().map_err(|e| e.to_string())?;
    let freelist = db.freelist().unwrap_or(0);
    let geo = info.geometry();

    let mut rows = Vec::new();
    let mut push = |g: &str, k: &str, v: String| rows.push((g.to_string(), k.to_string(), v));

    push("几何", "文件下限", human_size(geo.min_size()));
    push("几何", "文件上限", human_size(geo.max_size()));
    push("几何", "当前大小", human_size(geo.current_size()));
    push("几何", "增长步长", human_size(geo.growth_step()));
    push("几何", "收缩阈值", human_size(geo.shrink_threshold()));
    push("映射", "映射大小", human_size(info.map_size() as u64));
    push("映射", "已用页数", (info.last_pgno() + 1).to_string());
    push("映射", "空闲页数", freelist.to_string());
    push("事务", "最后事务 ID", info.last_txnid().to_string());
    push("读者", "读者槽位上限", info.max_readers().to_string());
    push("读者", "当前读者数", info.num_readers().to_string());
    push("主表", "页大小", human_size(stat.page_size() as u64));
    push("主表", "B+树深度", stat.depth().to_string());
    push("主表", "分支页", stat.branch_pages().to_string());
    push("主表", "叶子页", stat.leaf_pages().to_string());
    push("主表", "溢出页", stat.overflow_pages().to_string());
    push("主表", "条目数", stat.entries().to_string());

    Ok(rows)
}
