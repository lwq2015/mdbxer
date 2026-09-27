//! 生成测试用 MDBX 数据库（目录模式与单文件模式各一份）。
//!
//! 用法：`cargo run --example make_test_db`
//! 输出：testdata/dir_db（目录模式）、testdata/file_db.mdbx（单文件模式）

use libmdbx::{Database, DatabaseOptions, NoWriteMap, TableFlags, WriteFlags};
use std::path::Path;

fn main() -> libmdbx::Result<()> {
    let base = Path::new("testdata");
    std::fs::create_dir_all(base).ok();

    // 目录模式：传入目录路径，mdbx 在内部创建 data.mdbx
    let dir_db = base.join("dir_db");
    std::fs::create_dir_all(&dir_db).ok();
    build(&dir_db, false)?;

    // 单文件模式（MDBX_NOSUBDIR）
    let file_db = base.join("file_db.mdbx");
    if file_db.exists() {
        std::fs::remove_file(&file_db).ok();
    }
    build(&file_db, true)?;

    println!("已生成：\n  {}\n  {}", dir_db.display(), file_db.display());
    Ok(())
}

fn build(path: &Path, no_sub_dir: bool) -> libmdbx::Result<()> {
    let options = DatabaseOptions {
        max_tables: Some(64),
        no_sub_dir,
        ..Default::default()
    };
    let db = Database::<NoWriteMap>::open_with_options(path, options)?;
    let txn = db.begin_rw_txn()?;

    // 1) 普通表：混合各种 value 形态
    let t = txn.create_table(Some("kv_basic"), TableFlags::default())?;
    let now_ms: u64 = 1_759_000_000_000; // 毫秒时间戳
    let now_s: u64 = 1_759_000_000; // 秒时间戳
    let entries: Vec<(&[u8], Vec<u8>)> = vec![
        (b"hello", b"world".to_vec()),
        ("中文键".as_bytes(), "中文值：你好，MDBX！".as_bytes().to_vec()),
        (b"empty_value", Vec::new()),
        (b"ts_seconds", now_s.to_le_bytes().to_vec()),
        (b"ts_millis", now_ms.to_le_bytes().to_vec()),
        (b"pi", 3.14159265358979f64.to_le_bytes().to_vec()),
        (b"ratio", 0.618f32.to_le_bytes().to_vec()),
        (b"count_u32", 42u32.to_le_bytes().to_vec()),
        (b"small_u16", 7u16.to_le_bytes().to_vec()),
        (b"neg_i32", (-12345i32).to_le_bytes().to_vec()),
        (b"big_blob", (0u8..=255).cycle().take(4096).collect()),
        (
            b"utf16_text",
            "UTF-16 文本"
                .encode_utf16()
                .flat_map(|u| u.to_le_bytes())
                .collect(),
        ),
    ];
    for (k, v) in entries {
        txn.put(&t, k, &v, WriteFlags::default())?;
    }

    // 2) 多值表（DUPSORT）：一个 key 挂 300 个值，测试多值分页
    let t = txn.create_table(Some("dup_multi"), TableFlags::DUP_SORT)?;
    for i in 0..300u32 {
        txn.put(
            &t,
            b"fruits",
            format!("value_{i:04}").as_bytes(),
            WriteFlags::default(),
        )?;
    }
    for i in 0..5u32 {
        txn.put(
            &t,
            b"colors",
            format!("color_{i}").as_bytes(),
            WriteFlags::default(),
        )?;
    }
    txn.put(&t, b"single", b"only_one", WriteFlags::default())?;

    // 3) 整数键表（INTEGER_KEY）：u64 LE key
    let t = txn.create_table(Some("int_keys"), TableFlags::INTEGER_KEY)?;
    for i in 1..=100u64 {
        txn.put(
            &t,
            i.to_le_bytes(),
            format!("第 {i} 条整数键记录").as_bytes(),
            WriteFlags::default(),
        )?;
    }

    // 4) 大表：20000 条，测试分页
    let t = txn.create_table(Some("big_table"), TableFlags::default())?;
    for i in 0..20_000u32 {
        txn.put(
            &t,
            format!("key_{i:06}").as_bytes(),
            format!("value_{i} —— 一些用于填充的中文内容").as_bytes(),
            WriteFlags::default(),
        )?;
    }

    txn.commit()?;
    Ok(())
}
