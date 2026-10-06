// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 分页读取、跳转定位与多值（DUP_SORT）读取。
//!
//! 多值表在表格中**按 Key 分组**：一个 Key 只占一行（值列表在右侧详情中翻看），
//! 用 MDBX 的 NEXT_NODUP/PREV_NODUP 在 Key 之间跳跃，避免某个 Key 值过多时
//! 整页都困在同一个 Key。

use core::ffi::c_int;
use libmdbx::{Database, NoWriteMap};

/// 一行表格数据。
#[derive(Clone, Debug)]
pub struct Row {
    /// 原始键字节
    pub key: Vec<u8>,
    /// 多值表分组行中为"代表值"（该 Key 的第一个值），普通表即唯一值。
    pub value: Vec<u8>,
    /// 多值表分组行：该 Key 的值总数；普通表为 None。
    pub dup_count: Option<usize>,
}

/// 取值方向（Cursor 遍历顺序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// 升序（first → next/next_nodup）
    Forward,
    /// 降序（last → prev/prev_nodup）
    Backward,
}

/// 分页锚点：按 Key 定位（多值表落在该 Key 的第一个值）。
/// 第二个元素为多值表的精确 value 锚点（当前未使用）。
pub type Anchor = (Vec<u8>, Option<Vec<u8>>);

/// 一页数据，rows 按取值方向排列。
pub struct Page {
    /// 页内行（最多 `limit` 条）
    pub rows: Vec<Row>,
    /// 取值方向上是否还有数据
    pub has_more: bool,
}

/// 跳转目标的 key 形式。
pub enum JumpKey {
    /// 任意字节序列
    Bytes(Vec<u8>),
    /// INTEGER_KEY 表：内部自动尝试 8/4 字节 LE
    Int(u64),
}

/// 游标当前项：`(key_bytes, value_bytes)` 或 `None`（已越界）。
type CursorItem = Option<(Vec<u8>, Vec<u8>)>;

/// 当前游标所在 Key 的重复值数量（仅 DUP_SORT 表有意义）。
///
/// libmdbx-rs 的安全封装没有提供 `mdbx_cursor_count`，这里通过它导出的
/// 原始游标句柄直接 FFI。该调用不移动游标位置；调用点与安全 API 在同一
/// 线程串行使用，不存在并发。
fn dup_count_at(cursor: &mut libmdbx::Cursor<'_, libmdbx::RO>) -> Option<usize> {
    unsafe extern "C" {
        fn mdbx_cursor_count(cursor: *const core::ffi::c_void, count: *mut usize) -> c_int;
    }
    let mut n = 0usize;
    // SAFETY: 游标句柄在本事务内有效；count 是有效的可写 usize。
    let rc = unsafe { mdbx_cursor_count(cursor.cursor().0.cast_const().cast(), &mut n) };
    (rc == 0).then_some(n)
}

/// 沿取值方向移动到下一条记录。
/// 多值表分组模式使用 nodup 原语：一次跨过当前 Key 的其余值，到下一个 Key。
///
/// - `grouped`：true 时多值表按 Key 分组（用 next_nodup/prev_nodup）
fn step(
    cursor: &mut libmdbx::Cursor<'_, libmdbx::RO>,
    grouped: bool,
    dir: Direction,
) -> Result<CursorItem, String> {
    let r = match (dir, grouped) {
        (Direction::Forward, false) => cursor.next(),
        (Direction::Backward, false) => cursor.prev(),
        (Direction::Forward, true) => cursor.next_nodup(),
        (Direction::Backward, true) => cursor.prev_nodup(),
    };
    r.map_err(|e| e.to_string())
}

/// 把游标当前位置的记录构造成一个分组行（取值数量、统一代表值）。
///
/// - `value`：游标当前值；多值表降序时会用 first_dup 换回首值
fn make_row(
    cursor: &mut libmdbx::Cursor<'_, libmdbx::RO>,
    grouped: bool,
    dir: Direction,
    key: Vec<u8>,
    mut value: Vec<u8>,
) -> Row {
    let dup_count = if grouped {
        // 降序时游标停在该 Key 的最后一个值，统一回到第一个值作为代表，
        // 保证无论向哪个方向翻页，同一 Key 的预览内容一致。
        if dir == Direction::Backward {
            if let Ok(Some(v0)) = cursor.first_dup::<Vec<u8>>() {
                value = v0;
            }
        }
        dup_count_at(cursor)
    } else {
        None
    };
    Row {
        key,
        value,
        dup_count,
    }
}

/// 从 `first` 开始沿 `dir` 方向收集最多 `limit` 条。
///
/// - `first`：起始项；None 表示表已空
/// - `prefix`：Some 时仅收取以该字节串开头的 Key；遇到首个不匹配前缀的
///   Key 即结束且 `has_more=false`（前缀区间连续，其后再无匹配项）
fn collect(
    cursor: &mut libmdbx::Cursor<'_, libmdbx::RO>,
    grouped: bool,
    dir: Direction,
    prefix: Option<&[u8]>,
    first: CursorItem,
    limit: usize,
) -> Result<Page, String> {
    let mut rows: Vec<Row> = Vec::with_capacity(limit.min(1024));
    let mut item = first;
    let mut prefix_out = false;
    // 多取 1 条用于判断 has_more
    while let Some((key, value)) = item {
        if let Some(p) = prefix {
            if !key.starts_with(p) {
                prefix_out = true;
                break;
            }
        }
        rows.push(make_row(cursor, grouped, dir, key, value));
        if rows.len() > limit {
            break;
        }
        item = step(cursor, grouped, dir)?;
    }
    let has_more = !prefix_out && rows.len() > limit;
    rows.truncate(limit);
    Ok(Page { rows, has_more })
}

/// 从锚点（含）或表首/尾开始取一页，可附加 Key 前缀约束。
///
/// - `prefix: Some` → 仅返回以该字节串开头的 Key：升序从 `set_lowerbound(prefix)`
///   起步，降序从 `set_upperbound(prefix ++ 0xFF×64)`（前缀区间末位）起步；
///   遍历时遇到首个不匹配前缀的 Key 即结束。前缀约束对 INTEGER_KEY 等表
///   按**字节语义**生效。
/// - `table`：None = 主表
/// - `dup_sort`：true 时多值表按 Key 分组；false 逐条遍历
/// - `anchor: None` → 从表首（Forward）或表尾（Backward）/前缀区间两端开始；
/// - `anchor: Some` → 用 set_lowerbound 定位到锚点 Key（多值表落在其第一个值，
///   Key 不存在则落在其后第一个 Key）；
/// - `skip_anchor: true` → 跳过锚点 Key 本身（用于"下一页/上一页"）。
/// - `limit`：页大小（最多取 limit 条）
pub fn fetch_page_prefix(
    db: &Database<NoWriteMap>,
    table: Option<&str>,
    dup_sort: bool,
    dir: Direction,
    prefix: Option<&[u8]>,
    anchor: Option<&Anchor>,
    skip_anchor: bool,
    limit: usize,
) -> Result<Page, String> {
    let txn = db.begin_ro_txn().map_err(|e| e.to_string())?;
    let table = txn.open_table(table).map_err(|e| e.to_string())?;
    let mut cursor = txn.cursor(&table).map_err(|e| e.to_string())?;

    let first: CursorItem = match anchor {
        None => match (dir, prefix) {
            (Direction::Forward, None) => cursor.first().map_err(|e| e.to_string())?,
            (Direction::Backward, None) => cursor.last().map_err(|e| e.to_string())?,
            (Direction::Forward, Some(p)) => cursor
                .set_lowerbound::<Vec<u8>, Vec<u8>>(p, None)
                .map_err(|e| e.to_string())?
                .map(|(_, k, v)| (k, v)),
            (Direction::Backward, Some(p)) => {
                // 前缀区间末位：≤ prefix ++ 0xFF.. 的最后一项即前缀内末位
                let mut end = p.to_vec();
                end.extend_from_slice(&[0xFF; 64]);
                cursor
                    .set_upperbound::<Vec<u8>, Vec<u8>>(&end)
                    .map_err(|e| e.to_string())?
            }
        },
        // 分组模式锚点只按 Key：多值表 None 表示落在该 Key 的第一个值。
        Some((key, _)) => cursor
            .set_lowerbound::<Vec<u8>, Vec<u8>>(key, None)
            .map_err(|e| e.to_string())?
            .map(|(_, k, v)| (k, v)),
    };
    let first = if skip_anchor && first.is_some() {
        step(&mut cursor, dup_sort, dir)?
    } else {
        first
    };
    collect(&mut cursor, dup_sort, dir, prefix, first, limit)
}

/// 按方向定位到 `key`：升序用 set_lowerbound（≥key 首项），降序用 set_upperbound（≤key 末项）。
fn jump_bytes(
    cursor: &mut libmdbx::Cursor<'_, libmdbx::RO>,
    dir: Direction,
    key: &[u8],
) -> Result<CursorItem, String> {
    match dir {
        Direction::Forward => cursor
            .set_lowerbound::<Vec<u8>, Vec<u8>>(key, None)
            .map(|o| o.map(|(_, k, v)| (k, v)))
            .map_err(|e| e.to_string()),
        Direction::Backward => cursor
            .set_upperbound::<Vec<u8>, Vec<u8>>(key)
            .map_err(|e| e.to_string()),
    }
}

/// 跳转到指定 key，以定位处为页首取一页。
///
/// - `table`：None = 主表
/// - `dup_sort`：true 时多值表按 Key 分组
/// - `key`：见 [`JumpKey`]；INTEGER_KEY 表用 `JumpKey::Int`（内部自动尝试 8/4 字节 LE）
/// - `limit`：页大小
pub fn jump_to(
    db: &Database<NoWriteMap>,
    table: Option<&str>,
    dup_sort: bool,
    dir: Direction,
    key: JumpKey,
    limit: usize,
) -> Result<Page, String> {
    let txn = db.begin_ro_txn().map_err(|e| e.to_string())?;
    let table = txn.open_table(table).map_err(|e| e.to_string())?;
    let mut cursor = txn.cursor(&table).map_err(|e| e.to_string())?;

    let first: CursorItem = match key {
        JumpKey::Bytes(bytes) => jump_bytes(&mut cursor, dir, &bytes)?,
        JumpKey::Int(v) => {
            // INTEGER_KEY：先试 u64 LE，长度不符再试 u32 LE
            match jump_bytes(&mut cursor, dir, &v.to_le_bytes()) {
                Ok(item) => item,
                Err(_) => jump_bytes(&mut cursor, dir, &(v as u32).to_le_bytes())?,
            }
        }
    };
    collect(&mut cursor, dup_sort, dir, None, first, limit)
}

/// 读取某个 key 的多值：返回 (值总数, 指定页的值列表)。
///
/// 总数用 `mdbx_cursor_count` 直接获取（不必遍历全部值）；
/// 值列表按页遍历（每页 DUP_PAGE_SIZE 个，在调用方设定）。
///
/// - `page_index`：0 起的页号
/// - `page_size`：每页值个数
pub fn dups_of(
    db: &Database<NoWriteMap>,
    table: Option<&str>,
    key: &[u8],
    page_index: usize,
    page_size: usize,
) -> Result<(usize, Vec<Vec<u8>>), String> {
    let txn = db.begin_ro_txn().map_err(|e| e.to_string())?;
    let table = txn.open_table(table).map_err(|e| e.to_string())?;
    let mut cursor = txn.cursor(&table).map_err(|e| e.to_string())?;

    // MDBX_SET：精确定位到该 Key（其第一个值）；Key 不存在则总数 0。
    let exists = cursor
        .set::<Vec<u8>>(key)
        .map_err(|e| e.to_string())?
        .is_some();
    let total = if exists {
        dup_count_at(&mut cursor).unwrap_or(0)
    } else {
        0
    };

    let start = page_index * page_size;
    let mut values = Vec::new();
    if exists {
        for (i, item) in cursor.iter_dup_of::<Vec<u8>, Vec<u8>>(key).enumerate() {
            if i >= start + page_size {
                break;
            }
            let (_, value) = item.map_err(|e| e.to_string())?;
            if i >= start {
                values.push(value);
            }
        }
    }
    Ok((total, values))
}

/// 导出用原始 KV 流的续读锚点：精确到 (key, value)，多值表可落在具体值上。
pub type RawAnchor = (Vec<u8>, Vec<u8>);

/// 一批原始 KV 记录（多值表逐值展开，不做 Key 分组）。
pub struct RawBatch {
    /// 本批记录（已截断到 limit 条），按遍历方向排列
    pub rows: Vec<(Vec<u8>, Vec<u8>)>,
    /// 遍历方向上是否还有数据
    pub has_more: bool,
}

/// 导出专用：按原始 (key, value) 流取一批（多值表逐值展开）。
///
/// MDBX 环境在同一进程内不允许二次 `open`（MDBX_BUSY），因此导出不能在
/// 工作线程里重开环境；由 UI 线程持有唯一环境句柄，分批读取后交给写盘线程。
///
/// - `dup_sort`：多值表用精确 (key, value) 锚点续读（普通表锚点只按 Key）
/// - `anchor: None` 从表首/表尾开始
/// - `skip_anchor: true` 跳过锚点记录本身（下一批用）
/// - 多取 1 条探针判断 `has_more`
pub fn fetch_raw_batch(
    db: &Database<NoWriteMap>,
    table: Option<&str>,
    dup_sort: bool,
    dir: Direction,
    anchor: Option<&RawAnchor>,
    skip_anchor: bool,
    limit: usize,
) -> Result<RawBatch, String> {
    let txn = db.begin_ro_txn().map_err(|e| e.to_string())?;
    let table = txn.open_table(table).map_err(|e| e.to_string())?;
    let mut cursor = txn.cursor(&table).map_err(|e| e.to_string())?;

    // 定位到批首（含锚点）
    let mut pos: CursorItem = match anchor {
        None => {
            if dir == Direction::Forward {
                cursor.first().map_err(|e| e.to_string())?
            } else {
                cursor.last().map_err(|e| e.to_string())?
            }
        }
        Some((k, v)) => {
            if dir == Direction::Forward {
                if dup_sort {
                    // ≥ (k, v) 的第一个值（锚点存在时即锚点本身）
                    cursor
                        .get_both_range::<Vec<u8>>(k, v)
                        .map_err(|e| e.to_string())?
                        .map(|val| (k.clone(), val))
                } else {
                    cursor
                        .set_lowerbound::<Vec<u8>, Vec<u8>>(k, None)
                        .map_err(|e| e.to_string())?
                        .map(|(_, kk, vv)| (kk, vv))
                }
            } else if dup_sort {
                // ≤ (k, v)：先到 ≥v 首值（锚点存在时即锚点本身），跳过再退一格
                match cursor
                    .get_both_range::<Vec<u8>>(k, v)
                    .map_err(|e| e.to_string())?
                {
                    Some(val) => Some((k.clone(), val)),
                    // 锚点已不在（数据被并发删除）：回退到 ≤k 末项
                    None => cursor
                        .set_upperbound::<Vec<u8>, Vec<u8>>(k)
                        .map_err(|e| e.to_string())?,
                }
            } else {
                cursor
                    .set_upperbound::<Vec<u8>, Vec<u8>>(k)
                    .map_err(|e| e.to_string())?
            }
        }
    };
    // 续读跳过锚点记录本身：升序 next；降序多值表 prev_dup（首值跨前一 Key），普通表 prev
    if skip_anchor && pos.is_some() {
        pos = if dir == Direction::Forward {
            cursor.next().map_err(|e| e.to_string())?
        } else if dup_sort {
            match cursor.prev_dup().map_err(|e| e.to_string())? {
                Some(item) => Some(item),
                None => cursor.prev_nodup().map_err(|e| e.to_string())?,
            }
        } else {
            cursor.prev().map_err(|e| e.to_string())?
        };
    }

    let mut rows: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(limit.min(1024));
    let mut item = pos;
    // 多取 1 条探针判断 has_more
    while let Some(pair) = item {
        rows.push(pair);
        if rows.len() > limit {
            break;
        }
        item = if dir == Direction::Forward {
            cursor.next()
        } else {
            cursor.prev()
        }
        .map_err(|e| e.to_string())?;
    }
    let has_more = rows.len() > limit;
    rows.truncate(limit);
    Ok(RawBatch { rows, has_more })
}

/// 在某个 key 的值列表中按**字节子串**搜索。
///
/// - `needle`：搜索子串（字节）
/// - `from_index`：搜索起始序号；forward 时含，backward 时不含
/// - `forward=true`：从 `from_index`（含）起向后找第一个命中；找不到则回绕到开头。
/// - `forward=false`：从 `from_index`（不含）起向前找最近命中；找不到则回绕到末尾。
///
/// 一次遍历同时维护"主方向结果"与"全局首/末命中"（回绕用），
/// 返回 `(序号, 值)`；无匹配返回 None。
pub fn dup_find(
    db: &Database<NoWriteMap>,
    table: Option<&str>,
    key: &[u8],
    needle: &[u8],
    from_index: usize,
    forward: bool,
) -> Result<Option<(usize, Vec<u8>)>, String> {
    let txn = db.begin_ro_txn().map_err(|e| e.to_string())?;
    let table = txn.open_table(table).map_err(|e| e.to_string())?;
    let mut cursor = txn.cursor(&table).map_err(|e| e.to_string())?;

    if cursor
        .set::<Vec<u8>>(key)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Ok(None);
    }
    let hit = |v: &[u8]| v.windows(needle.len()).any(|w| w == needle);

    let mut main_hit: Option<(usize, Vec<u8>)> = None;
    let mut wrap_hit: Option<(usize, Vec<u8>)> = None;
    for (i, item) in cursor.iter_dup_of::<Vec<u8>, Vec<u8>>(key).enumerate() {
        let (_, v) = item.map_err(|e| e.to_string())?;
        if !hit(&v) {
            continue;
        }
        if forward {
            // 主区域首个命中直接返回；i < from 的首个命中留作回绕
            if i >= from_index {
                return Ok(Some((i, v)));
            }
            wrap_hit.get_or_insert((i, v));
        } else {
            // i < from 中最大的命中（不断覆盖）；全局最大命中留作回绕
            if i < from_index {
                main_hit = Some((i, v.clone()));
            }
            wrap_hit = Some((i, v));
        }
    }
    Ok(main_hit.or(wrap_hit))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MDBX 同一进程内不允许并发二次 open（MDBX_BUSY）：
    /// 打开真实测试库的测试共用这把锁串行执行。
    static REAL_DB_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock_real_db() -> std::sync::MutexGuard<'static, ()> {
        REAL_DB_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn row_clone_debug() {
        let row = Row {
            key: vec![1, 2, 3],
            value: vec![4, 5],
            dup_count: Some(10),
        };
        let cloned = row.clone();
        assert_eq!(cloned.key, vec![1, 2, 3]);
        assert_eq!(cloned.value, vec![4, 5]);
        assert_eq!(cloned.dup_count, Some(10));
        // Debug 不 panic
        let _ = format!("{:?}", row);
    }

    #[test]
    fn direction_copy_eq() {
        let f = Direction::Forward;
        let b = Direction::Backward;
        assert_ne!(f, b);
        assert_eq!(f, Direction::Forward);
    }

    #[test]
    fn jump_key_variants() {
        let k1 = JumpKey::Bytes(vec![0x41, 0x42]);
        let k2 = JumpKey::Int(42);
        // 仅验证构造不 panic
        match k1 {
            JumpKey::Bytes(b) => assert_eq!(b, vec![0x41, 0x42]),
            _ => panic!("expected Bytes"),
        }
        match k2 {
            JumpKey::Int(v) => assert_eq!(v, 42),
            _ => panic!("expected Int"),
        }
    }

    #[test]
    fn page_fields() {
        let page = Page {
            rows: vec![Row {
                key: vec![],
                value: vec![],
                dup_count: None,
            }],
            has_more: true,
        };
        assert_eq!(page.rows.len(), 1);
        assert!(page.has_more);
    }

    #[test]
    fn anchor_type() {
        let anchor: Anchor = (vec![1, 2], Some(vec![3]));
        assert_eq!(anchor.0, vec![1, 2]);
        assert_eq!(anchor.1, Some(vec![3]));
    }

    /// 导出分批在真实测试库上的端到端校验（需先生成 testdata：
    /// `cargo run --example make_test_db`；无测试库时自动跳过）。
    #[test]
    fn raw_batch_pagination_on_real_db() {
        let _g = lock_real_db();
        let db_path = std::path::Path::new("testdata/dir_db");
        if !db_path.exists() {
            eprintln!("跳过：testdata/dir_db 不存在（cargo run --example make_test_db 生成）");
            return;
        }
        let handle = crate::db::DbHandle::open(db_path, crate::db::OpenMode::Auto).unwrap();

        // 普通表 kv_basic：小批多分，拼接应等于整体，且无重复无遗漏
        for dir in [Direction::Forward, Direction::Backward] {
            let mut all: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
            let mut anchor: Option<RawAnchor> = None;
            loop {
                let b = fetch_raw_batch(
                    &handle.db,
                    Some("kv_basic"),
                    false,
                    dir,
                    anchor.as_ref(),
                    anchor.is_some(),
                    10,
                )
                .unwrap();
                let n = b.rows.len();
                all.extend(b.rows);
                if !b.has_more {
                    break;
                }
                anchor = all.last().cloned();
                assert!(n >= 10, "probe must fill batches until the last");
            }
            let total = handle
                .tables
                .iter()
                .find(|t| t.name.as_deref() == Some("kv_basic"))
                .unwrap()
                .entries;
            assert_eq!(all.len(), total, "dir {dir:?} count mismatch");
            // 无重复键
            let mut keys: Vec<Vec<u8>> = all.iter().map(|(k, _)| k.clone()).collect();
            let before = keys.len();
            keys.dedup();
            assert_eq!(keys.len(), before, "dup key in dir {dir:?}");
            // 顺序正确：升序单调不减，降序单调不增
            for w in keys.windows(2) {
                if dir == Direction::Forward {
                    assert!(w[0] <= w[1], "forward order broken");
                } else {
                    assert!(w[0] >= w[1], "backward order broken");
                }
            }
        }

        // 多值表 dup_multi：逐值流总数 = entries（值对数），同 Key 连续
        let mut all: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        let mut anchor: Option<RawAnchor> = None;
        loop {
            let b = fetch_raw_batch(
                &handle.db,
                Some("dup_multi"),
                true,
                Direction::Forward,
                anchor.as_ref(),
                anchor.is_some(),
                100,
            )
            .unwrap();
            all.extend(b.rows);
            if !b.has_more {
                break;
            }
            anchor = all.last().cloned();
        }
        let total = handle
            .tables
            .iter()
            .find(|t| t.name.as_deref() == Some("dup_multi"))
            .unwrap()
            .entries;
        assert_eq!(all.len(), total);
        // 同 Key 的值必须连续成组
        let mut seen = std::collections::HashSet::new();
        let mut prev: Option<&Vec<u8>> = None;
        for (k, _) in &all {
            if Some(k) != prev {
                assert!(seen.insert(k.clone()), "key {k:?} appears in 2 groups");
                prev = Some(k);
            }
        }

        // 多值表降序分批：同样总数一致 + 单调不增
        let mut all: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        let mut anchor: Option<RawAnchor> = None;
        loop {
            let b = fetch_raw_batch(
                &handle.db,
                Some("dup_multi"),
                true,
                Direction::Backward,
                anchor.as_ref(),
                anchor.is_some(),
                128,
            )
            .unwrap();
            all.extend(b.rows);
            if !b.has_more {
                break;
            }
            anchor = all.last().cloned();
        }
        assert_eq!(all.len(), total);
        for w in all.windows(2) {
            assert!(w[0].0 >= w[1].0, "dup backward key order broken");
        }
    }

    /// 全表 Value 搜索的端到端校验：分批扫描 + 文本匹配 + jump_to 定位。
    /// 需先生成 testdata（`cargo run --example make_test_db`），无则跳过。
    #[test]
    fn full_value_search_scan_and_locate() {
        let _g = lock_real_db();
        let db_path = std::path::Path::new("testdata/dir_db");
        if !db_path.exists() {
            eprintln!("跳过：testdata/dir_db 不存在（cargo run --example make_test_db 生成）");
            return;
        }
        let handle = crate::db::DbHandle::open(db_path, crate::db::OpenMode::Auto).unwrap();

        // 与 UI poll_value_search 相同的扫描逻辑：小批量扫描 kv_basic，
        // 找 value 显示文本（Auto 解码）包含 "embedded" 的记录
        let needle = "embedded".to_lowercase();
        let mut found: Option<(Vec<u8>, Vec<u8>)> = None;
        let mut anchor: Option<RawAnchor> = None;
        let mut scanned = 0usize;
        while found.is_none() {
            let b = fetch_raw_batch(
                &handle.db,
                Some("kv_basic"),
                false,
                Direction::Forward,
                anchor.as_ref(),
                anchor.is_some(),
                3,
            )
            .unwrap();
            scanned += b.rows.len();
            for (k, v) in &b.rows {
                let text = crate::fmt::decode(
                    v,
                    crate::fmt::DecodeMode::Auto,
                    crate::fmt::Endian::Little,
                    65536,
                )
                .to_lowercase();
                if text.contains(&needle) {
                    found = Some((k.clone(), v.clone()));
                    break;
                }
            }
            if found.is_none() {
                assert!(b.has_more, "扫完 {scanned} 条仍无匹配");
                anchor = b.rows.last().cloned();
            }
        }

        // 定位：jump_to 必须落在匹配 key 所在页
        let (k, _) = found.unwrap();
        let page = jump_to(
            &handle.db,
            Some("kv_basic"),
            false,
            Direction::Forward,
            JumpKey::Bytes(k.clone()),
            10,
        )
        .unwrap();
        assert!(!page.rows.is_empty());
        assert_eq!(page.rows[0].key, k, "jump_to 应落在匹配 key 上");

        // 多值表：搜索展开后的值（value_0299），定位到所属 key 分组页
        let mut found: Option<Vec<u8>> = None;
        let mut anchor: Option<RawAnchor> = None;
        while found.is_none() {
            let b = fetch_raw_batch(
                &handle.db,
                Some("dup_multi"),
                true,
                Direction::Forward,
                anchor.as_ref(),
                anchor.is_some(),
                128,
            )
            .unwrap();
            for (k, v) in &b.rows {
                if String::from_utf8_lossy(v).contains("value_0299") {
                    found = Some(k.clone());
                    break;
                }
            }
            if found.is_none() {
                assert!(b.has_more, "dup_multi 扫完仍无匹配");
                anchor = b.rows.last().cloned();
            }
        }
        let page = jump_to(
            &handle.db,
            Some("dup_multi"),
            true,
            Direction::Forward,
            JumpKey::Bytes(found.unwrap()),
            10,
        )
        .unwrap();
        assert!(!page.rows.is_empty());
        assert_eq!(page.rows[0].dup_count, Some(300), "应落在 fruits 分组行");
    }
}
