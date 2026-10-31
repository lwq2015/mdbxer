// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 环境打开与 subDB 枚举。

use libmdbx::{Database, DatabaseOptions, Mode, NoWriteMap, ReadWriteOptions, TableFlags};
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
        Self::open_with(path, mode, false)
    }

    /// 以读写模式打开环境以触发 MDBX 自动恢复（针对上次未正常关闭的库），
    /// 成功后立即关闭写句柄并重新以只读打开，回到纯只读语义。
    ///
    /// ⚠️ 仅在用户明确确认后调用：恢复会修改库文件。
    pub fn open_recover(path: &Path, mode: OpenMode) -> Result<Self, String> {
        // 读写句柄只用来触发恢复，必须在下一次打开前释放：libmdbx 禁止同进程
        // 对同一环境开两个句柄（MDBX_BUSY），所以这里显式 drop，而不是靠临时值
        // 在语句末尾自动析构——后者一旦被人改成绑定变量就会让重开失败。
        drop(Self::open_with(path, mode, true)?);
        Self::open(path, mode)
    }

    fn open_with(path: &Path, mode: OpenMode, rw: bool) -> Result<Self, String> {
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
            mode: if rw {
                // 读写打开即触发 MDBX 对未干净关闭环境的自动恢复
                Mode::ReadWrite(ReadWriteOptions::default())
            } else {
                Mode::ReadOnly
            },
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

    /// 重新枚举子表与条目数（不重新打开环境）。
    ///
    /// libmdbx 禁止同进程二次 open 同一环境（MDBX_BUSY），因此刷新只能复用现有
    /// 句柄在新只读事务里重跑 `list_tables`；数据读取本就每次新建事务，天然最新。
    pub fn refresh_tables(&mut self) -> Result<(), String> {
        self.tables = Self::list_tables(&self.db)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libmdbx::{Database, DatabaseOptions, Mode, NoWriteMap, ReadWriteOptions};

    /// 在系统临时目录创建一个最小空库（创建后立即关闭写句柄），返回路径。
    fn make_temp_db(tag: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("mdbxer_{}_{}.mdbx", tag, std::process::id()));
        let _ = std::fs::remove_file(&path);
        let opts = DatabaseOptions {
            no_sub_dir: true,
            mode: Mode::ReadWrite(ReadWriteOptions::default()),
            ..Default::default()
        };
        drop(Database::<NoWriteMap>::open_with_options(&path, opts).expect("create temp db"));
        path
    }

    /// 回归：libmdbx 禁止同进程对同一库开两个环境（MDBX_BUSY，经 LCK 共享内存
    /// 的 rthc_uniq_check 进程内唯一性检查实现）。open_db/run_recover 的重开
    /// 逻辑必须先关旧句柄——否则修复成功后的重新打开会被自己的临时句柄挡住，
    /// 表现为"修复了却打不开、重启程序才正常"。
    #[test]
    fn reopen_same_file_in_process_requires_closing_first() {
        let path = make_temp_db("busy_reopen");

        let first = DbHandle::open(&path, OpenMode::SingleFile).expect("首次打开应成功");

        // 同进程二次打开同一文件：应被拒绝，且错误码为 MDBX_BUSY
        let err = Database::<NoWriteMap>::open_with_options(
            &path,
            DatabaseOptions {
                no_sub_dir: true,
                accede: true,
                mode: Mode::ReadOnly,
                ..Default::default()
            },
        )
        .expect_err("同进程二次打开应被拒绝");
        assert!(
            matches!(err, libmdbx::Error::Busy),
            "应为 MDBX_BUSY，实际：{err:?}"
        );

        // 关闭旧句柄后即可重开（修复方案"先关再开"的依据）
        drop(first);
        let again = DbHandle::open(&path, OpenMode::SingleFile).expect("关闭旧句柄后重开应成功");
        drop(again);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("lck"));
    }
}
