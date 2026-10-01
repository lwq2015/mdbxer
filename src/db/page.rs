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
    let rc = unsafe {
        mdbx_cursor_count(cursor.cursor().0.cast_const().cast(), &mut n)
    };
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
fn collect(
    cursor: &mut libmdbx::Cursor<'_, libmdbx::RO>,
    grouped: bool,
    dir: Direction,
    first: CursorItem,
    limit: usize,
) -> Result<Page, String> {
    let mut rows: Vec<Row> = Vec::with_capacity(limit.min(1024));
    let mut item = first;
    // 多取 1 条用于判断 has_more
    while let Some((key, value)) = item {
        rows.push(make_row(cursor, grouped, dir, key, value));
        if rows.len() > limit {
            break;
        }
        item = step(cursor, grouped, dir)?;
    }
    let has_more = rows.len() > limit;
    rows.truncate(limit);
    Ok(Page { rows, has_more })
}

/// 从锚点（含）或表首/尾开始取一页。
///
/// - `table`：None = 主表
/// - `dup_sort`：true 时多值表按 Key 分组；false 逐条遍历
/// - `anchor: None` → 从表首（Forward）或表尾（Backward）开始；
/// - `anchor: Some` → 用 set_lowerbound 定位到锚点 Key（多值表落在其第一个值，
///   Key 不存在则落在其后第一个 Key）；
/// - `skip_anchor: true` → 跳过锚点 Key 本身（用于"下一页/上一页"）。
/// - `limit`：页大小（最多取 limit 条）
pub fn fetch_page(
    db: &Database<NoWriteMap>,
    table: Option<&str>,
    dup_sort: bool,
    dir: Direction,
    anchor: Option<&Anchor>,
    skip_anchor: bool,
    limit: usize,
) -> Result<Page, String> {
    let txn = db.begin_ro_txn().map_err(|e| e.to_string())?;
    let table = txn.open_table(table).map_err(|e| e.to_string())?;
    let mut cursor = txn.cursor(&table).map_err(|e| e.to_string())?;

    let first: CursorItem = match anchor {
        None => match dir {
            Direction::Forward => cursor.first().map_err(|e| e.to_string())?,
            Direction::Backward => cursor.last().map_err(|e| e.to_string())?,
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
    collect(&mut cursor, dup_sort, dir, first, limit)
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
    collect(&mut cursor, dup_sort, dir, first, limit)
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
        for (i, item) in cursor
            .iter_dup_of::<Vec<u8>, Vec<u8>>(key)
            .enumerate()
        {
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

/// 在某个 Key 的值列表中按**字节子串**搜索。
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
    for (i, item) in cursor
        .iter_dup_of::<Vec<u8>, Vec<u8>>(key)
        .enumerate()
    {
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
