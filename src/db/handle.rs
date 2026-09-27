//! 环境打开与 subDB 枚举。

use libmdbx::{Database, DatabaseOptions, Mode, NoWriteMap, TableFlags};
use std::path::Path;

use super::OpenMode;

/// 一个已打开的 MDBX 环境（只读）。
pub struct DbHandle {
    pub db: Database<NoWriteMap>,
    /// 实际生效的打开方式：true = 单文件（NOSUBDIR）
    pub no_sub_dir: bool,
    pub tables: Vec<TableInfo>,
}

/// 一张表（subDB）的概要信息。
#[derive(Clone)]
pub struct TableInfo {
    /// None 表示主表（未命名）
    pub name: Option<String>,
    /// 显示名（主表显示为"（主表）"）
    pub display: String,
    pub entries: usize,
    /// 人类可读的标志描述，如 "多值, 整数键"
    pub flags_desc: String,
    pub dup_sort: bool,
    pub integer_key: bool,
}

impl TableInfo {
    fn new(name: Option<String>, entries: usize, flags: TableFlags) -> Self {
        let mut desc = Vec::new();
        if flags.contains(TableFlags::DUP_SORT) {
            desc.push("多值");
        }
        if flags.contains(TableFlags::INTEGER_KEY) {
            desc.push("整数键");
        }
        if flags.contains(TableFlags::DUP_FIXED) {
            desc.push("定长多值");
        }
        if flags.contains(TableFlags::INTEGER_DUP) {
            desc.push("整数值");
        }
        if flags.contains(TableFlags::REVERSE_KEY) {
            desc.push("反序键");
        }
        if flags.contains(TableFlags::REVERSE_DUP) {
            desc.push("反序值");
        }
        Self {
            display: name.clone().unwrap_or_else(|| "（主表）".to_string()),
            name,
            entries,
            flags_desc: desc.join(", "),
            dup_sort: flags.contains(TableFlags::DUP_SORT),
            integer_key: flags.contains(TableFlags::INTEGER_KEY),
        }
    }
}

impl DbHandle {
    /// 以只读 + ACCEDE 方式打开环境（可查看正被其他进程使用的库）。
    pub fn open(path: &Path, mode: OpenMode) -> Result<Self, String> {
        if !path.exists() {
            return Err(format!("路径不存在：{}", path.display()));
        }
        let no_sub_dir = match mode {
            OpenMode::Auto => path.is_file(),
            OpenMode::SingleFile => true,
            OpenMode::Directory => false,
        };
        let options = DatabaseOptions {
            max_tables: Some(4096),
            no_sub_dir,
            accede: true,
            mode: Mode::ReadOnly,
            ..Default::default()
        };
        let db = Database::<NoWriteMap>::open_with_options(path, options)
            .map_err(|e| format!("打开环境失败：{e}"))?;
        let tables = Self::list_tables(&db)?;
        Ok(Self {
            db,
            no_sub_dir,
            tables,
        })
    }

    /// 枚举命名表：遍历主表 key 并逐个试探 open_table；
    /// 一个命名表都没有但主表有数据时，给出"（主表）"伪条目。
    fn list_tables(db: &Database<NoWriteMap>) -> Result<Vec<TableInfo>, String> {
        let txn = db.begin_ro_txn().map_err(|e| e.to_string())?;
        let main = txn.open_table(None).map_err(|e| e.to_string())?;
        let main_entries = txn
            .table_stat(&main)
            .map_err(|e| e.to_string())?
            .entries();

        let mut tables = Vec::new();
        let mut cursor = txn.cursor(&main).map_err(|e| e.to_string())?;
        let iter = cursor.iter_start::<Vec<u8>, Vec<u8>>();
        for item in iter {
            let (key, _) = item.map_err(|e| e.to_string())?;
            // 表名必须是合法 UTF-8 才能作为 open_table 的名字
            let Ok(name) = std::str::from_utf8(&key) else {
                continue;
            };
            let Ok(table) = txn.open_table(Some(name)) else {
                // 主表中的普通数据，不是命名表
                continue;
            };
            let stat = txn.table_stat(&table).map_err(|e| e.to_string())?;
            let flags = txn.table_flags(&table).map_err(|e| e.to_string())?;
            tables.push(TableInfo::new(
                Some(name.to_string()),
                stat.entries(),
                flags,
            ));
        }

        if tables.is_empty() && main_entries > 0 {
            let flags = txn.table_flags(&main).map_err(|e| e.to_string())?;
            tables.push(TableInfo::new(None, main_entries, flags));
        }
        Ok(tables)
    }
}
