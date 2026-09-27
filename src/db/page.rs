//! 分页读取、跳转定位与多值（DUP_SORT）读取。

use libmdbx::{Database, NoWriteMap};

/// 一行 KV 数据。
#[derive(Clone, Debug)]
pub struct Row {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

/// 取值方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// 升序（first → next）
    Forward,
    /// 降序（last → prev）
    Backward,
}

/// 分页锚点：(key, 多值表的 dup value)。value 仅 DUP_SORT 表使用。
pub type Anchor = (Vec<u8>, Option<Vec<u8>>);

/// 一页数据，rows 按取值方向排列。
pub struct Page {
    pub rows: Vec<Row>,
    /// 取值方向上是否还有数据
    pub has_more: bool,
}

/// 跳转目标的 key。
pub enum JumpKey {
    Bytes(Vec<u8>),
    /// INTEGER_KEY 表：内部自动尝试 8/4 字节 LE
    Int(u64),
}

type CursorItem = Option<(Vec<u8>, Vec<u8>)>;

fn step(
    cursor: &mut libmdbx::Cursor<'_, libmdbx::RO>,
    dir: Direction,
) -> Result<CursorItem, String> {
    match dir {
        Direction::Forward => cursor.next().map_err(|e| e.to_string()),
        Direction::Backward => cursor.prev().map_err(|e| e.to_string()),
    }
}

fn collect(
    cursor: &mut libmdbx::Cursor<'_, libmdbx::RO>,
    dir: Direction,
    first: CursorItem,
    limit: usize,
) -> Result<Page, String> {
    let mut rows: Vec<Row> = Vec::with_capacity(limit.min(1024));
    let mut item = first;
    // 多取 1 条用于判断 has_more
    while let Some((key, value)) = item {
        rows.push(Row { key, value });
        if rows.len() > limit {
            break;
        }
        item = step(cursor, dir)?;
    }
    let has_more = rows.len() > limit;
    rows.truncate(limit);
    Ok(Page { rows, has_more })
}

/// 从锚点（含）或表首/尾开始取一页。
///
/// - `anchor: None` → 从表首（Forward）或表尾（Backward）开始；
/// - `anchor: Some` → 用 set_lowerbound 定位到锚点（不存在则落在其后第一对）；
/// - `skip_anchor: true` → 跳过锚点本身（用于"下一页/上一页"）。
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
        Some((key, value)) => {
            let value = if dup_sort { value.as_deref() } else { None };
            cursor
                .set_lowerbound::<Vec<u8>, Vec<u8>>(key, value)
                .map_err(|e| e.to_string())?
                .map(|(_, k, v)| (k, v))
        }
    };
    let first = if skip_anchor && first.is_some() {
        step(&mut cursor, dir)?
    } else {
        first
    };
    collect(&mut cursor, dir, first, limit)
}

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
pub fn jump_to(
    db: &Database<NoWriteMap>,
    table: Option<&str>,
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
    collect(&mut cursor, dir, first, limit)
}

/// 读取某个 key 的多值：返回 (dup 总数, 指定页的值列表)。
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

    let start = page_index * page_size;
    let mut total = 0usize;
    let mut values = Vec::new();
    for item in cursor.iter_dup_of::<Vec<u8>, Vec<u8>>(key) {
        let (_, value) = item.map_err(|e| e.to_string())?;
        if total >= start && total < start + page_size {
            values.push(value);
        }
        total += 1;
    }
    Ok((total, values))
}

/// 求某个 value 在该 key 的多值列表中的序号（用于选中行后定位"第几个值"）。
pub fn dup_index_of(
    db: &Database<NoWriteMap>,
    table: Option<&str>,
    key: &[u8],
    value: &[u8],
) -> Result<Option<usize>, String> {
    let txn = db.begin_ro_txn().map_err(|e| e.to_string())?;
    let table = txn.open_table(table).map_err(|e| e.to_string())?;
    let mut cursor = txn.cursor(&table).map_err(|e| e.to_string())?;

    for (i, item) in cursor
        .iter_dup_of::<Vec<u8>, Vec<u8>>(key)
        .enumerate()
    {
        let (_, v) = item.map_err(|e| e.to_string())?;
        if v == value {
            return Ok(Some(i));
        }
    }
    Ok(None)
}
