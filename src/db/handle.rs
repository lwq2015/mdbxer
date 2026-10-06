// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 环境打开与 subDB 枚举。

use libmdbx::{Database, DatabaseOptions, Mode, NoWriteMap, TableFlags};
use std::path::Path;

use super::OpenMode;

/// 一个已打开的 MDBX 环境（只读）。
pub struct DbHandle {
    /// 底层只读数据库连接
    pub db: Database<NoWriteMap>,
    /// 实际生效的打开方式：true = 单文件（NOSUBDIR）
    pub no_sub_dir: bool,
    /// 主表 + 所有命名子表
    pub tables: Vec<TableInfo>,
}

/// 一张表（subDB）的概要信息。
#[derive(Clone)]
pub struct TableInfo {
    /// None 表示主表（未命名）
    pub name: Option<String>,
    /// 条目数（entries），主表包含命名表的名称记录
    pub entries: usize,
    /// 表标志原始位（显示文案按当前界面语言现算，见 `display`/`flags_desc`）
    flags: TableFlags,
    /// 是否 DUP_SORT（多值表）
    pub dup_sort: bool,
    /// 是否 INTEGER_KEY（键按 u64 LE 排序）
    pub integer_key: bool,
}

impl TableInfo {
    fn new(name: Option<String>, entries: usize, flags: TableFlags) -> Self {
        Self {
            name,
            entries,
            flags,
            dup_sort: flags.contains(TableFlags::DUP_SORT),
            integer_key: flags.contains(TableFlags::INTEGER_KEY),
        }
    }

    /// 显示名：命名表用原名，主表显示"（主表）"（随界面语言）。
    pub fn display(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| crate::i18n::tr().main_table.to_string())
    }

    /// 人类可读的标志描述，如 "多值, 整数键"（随界面语言；无标志返回空串）。
    pub fn flags_desc(&self) -> String {
        let t = crate::i18n::tr();
        let mut desc: Vec<&str> = Vec::new();
        if self.flags.contains(TableFlags::DUP_SORT) {
            desc.push(t.flag_dup_sort);
        }
        if self.flags.contains(TableFlags::INTEGER_KEY) {
            desc.push(t.flag_integer_key);
        }
        if self.flags.contains(TableFlags::DUP_FIXED) {
            desc.push(t.flag_dup_fixed);
        }
        if self.flags.contains(TableFlags::INTEGER_DUP) {
            desc.push(t.flag_integer_dup);
        }
        if self.flags.contains(TableFlags::REVERSE_KEY) {
            desc.push(t.flag_reverse_key);
        }
        if self.flags.contains(TableFlags::REVERSE_DUP) {
            desc.push(t.flag_reverse_dup);
        }
        desc.join(", ")
    }
}

impl DbHandle {
    /// 以只读 + ACCEDE 方式打开环境（可查看正被其他进程使用的库）。
    pub fn open(path: &Path, mode: OpenMode) -> Result<Self, String> {
        if !path.exists() {
            return Err(crate::i18n::tr().path_missing(&path.display().to_string()));
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
            .map_err(|e| crate::i18n::tr().open_env_fail(&e.to_string()))?;
        let tables = Self::list_tables(&db)?;
        Ok(Self {
            db,
            no_sub_dir,
            tables,
        })
    }

    /// 枚举表：未命名主表始终存在（固定排在第一位），
    /// 命名表通过遍历主表 key 逐个试探 open_table 得到。
    ///
    /// 注意：主表中每个命名表的名字也作为一条记录存在，
    /// 因此主表 entries 包含这些名字记录，浏览主表时同样可见（引擎真实内容）。
    fn list_tables(db: &Database<NoWriteMap>) -> Result<Vec<TableInfo>, String> {
        let txn = db.begin_ro_txn().map_err(|e| e.to_string())?;
        let main = txn.open_table(None).map_err(|e| e.to_string())?;
        let main_entries = txn.table_stat(&main).map_err(|e| e.to_string())?.entries();
        let main_flags = txn.table_flags(&main).map_err(|e| e.to_string())?;

        // 主表始终存在，固定排第一
        let mut tables = vec![TableInfo::new(None, main_entries, main_flags)];

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

        Ok(tables)
    }
}
