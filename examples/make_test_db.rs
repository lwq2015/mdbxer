// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 生成测试用 MDBX 数据库（目录模式与单文件模式各一份）。
//!
//! 用法：`cargo run --example make_test_db`
//! 输出：testdata/dir_db（目录模式）、testdata/file_db.mdbx（单文件模式）

// 测试数据有意使用 3.14159 / 1.41421356 这类"真实业务值"而非 consts::PI/SQRT_2
#![allow(clippy::approx_constant)]

use libmdbx::{Database, DatabaseOptions, NoWriteMap, TableFlags, WriteFlags};
use std::io::Write;
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
    let now_ms: u64 = 1_759_000_000_123; // 毫秒时间戳（非整秒，验证毫秒显示）
    let now_s: u64 = 1_759_000_000; // 秒时间戳
    // 整数/浮点全覆盖，键名统一为 `<排版>_<实际值>`：
    // 负数在值后加 `_neg` 后缀（如 i32_12345_neg 即 -12345），
    // 边界值用 min/max，补零样本加 pad 标注；数值均按 little-endian
    // 写入（顶栏切 BE 时同一字节会解释成另一个数）。
    let nums: Vec<(&[u8], Vec<u8>)> = vec![
        // ── uint8 / int8 ──
        (b"u8_0", 0u8.to_le_bytes().to_vec()),
        (b"u8_66", 66u8.to_le_bytes().to_vec()),
        (b"u8_max", u8::MAX.to_le_bytes().to_vec()),
        (b"i8_42", 42i8.to_le_bytes().to_vec()),
        (b"i8_5_neg", (-5i8).to_le_bytes().to_vec()),
        (b"i8_min", i8::MIN.to_le_bytes().to_vec()),
        (b"i8_max", i8::MAX.to_le_bytes().to_vec()),
        // ── uint16 / int16 ──
        (b"u16_0", 0u16.to_le_bytes().to_vec()),
        (b"u16_7", 7u16.to_le_bytes().to_vec()),
        (b"u16_max", u16::MAX.to_le_bytes().to_vec()),
        (b"i16_1_neg", (-1i16).to_le_bytes().to_vec()),
        (b"i16_min", i16::MIN.to_le_bytes().to_vec()),
        (b"i16_max", i16::MAX.to_le_bytes().to_vec()),
        // ── uint32 / int32 ──
        (b"u32_0", 0u32.to_le_bytes().to_vec()),
        (b"u32_42", 42u32.to_le_bytes().to_vec()),
        (b"u32_max", u32::MAX.to_le_bytes().to_vec()),
        (b"i32_12345_neg", (-12345i32).to_le_bytes().to_vec()),
        (b"i32_min", i32::MIN.to_le_bytes().to_vec()),
        (b"i32_max", i32::MAX.to_le_bytes().to_vec()),
        // ── uint64 / int64 ──
        (b"u64_0", 0u64.to_le_bytes().to_vec()),
        // 2024 不落在时间戳区间，验证普通 u64 不附日期
        (b"u64_2024", 2024u64.to_le_bytes().to_vec()),
        (b"u64_max", u64::MAX.to_le_bytes().to_vec()),
        (
            b"i64_9000000000000_neg",
            (-9_000_000_000_000i64).to_le_bytes().to_vec(),
        ),
        (b"i64_min", i64::MIN.to_le_bytes().to_vec()),
        (b"i64_max", i64::MAX.to_le_bytes().to_vec()),
        // 实际值 1，但只有 3 字节：验证按 i64/u64 解释时零扩展（应标"（补零）"）
        (b"i64_1_pad3", vec![0x01, 0x00, 0x00]),
        // ── float / double ──
        (b"f32_0", 0.0f32.to_le_bytes().to_vec()),
        (b"f32_1", 1.0f32.to_le_bytes().to_vec()),
        (b"f32_2.5_neg", (-2.5f32).to_le_bytes().to_vec()),
        (b"f32_0.618", 0.618f32.to_le_bytes().to_vec()),
        (b"f32_1e20", 1e20f32.to_le_bytes().to_vec()),
        // 整数部分 ≥5 位：验证浮点数千位分隔（应显示 123,456.75）
        (b"f32_123456.75", 123456.75f32.to_le_bytes().to_vec()),
        (b"f64_0", 0.0f64.to_le_bytes().to_vec()),
        (
            b"f64_1.41421356_neg",
            (-1.41421356f64).to_le_bytes().to_vec(),
        ),
        (b"f64_3.14159265", 3.14159265f64.to_le_bytes().to_vec()),
        (b"f64_1e100", 1e100f64.to_le_bytes().to_vec()),
        // 整数部分长：应显示 123,456,789.25 / -9,876,543,210.5
        (b"f64_123456789.25", 123456789.25f64.to_le_bytes().to_vec()),
        (
            b"f64_9876543210.5_neg",
            (-9876543210.5f64).to_le_bytes().to_vec(),
        ),
    ];
    let entries: Vec<(&[u8], Vec<u8>)> = vec![
        (b"hello", b"world".to_vec()),
        (
            "中文键".as_bytes(),
            "中文值：你好，MDBX！".as_bytes().to_vec(),
        ),
        (b"empty_value", Vec::new()),
        (b"ts_seconds", now_s.to_le_bytes().to_vec()),
        (b"ts_millis", now_ms.to_le_bytes().to_vec()),
        (b"big_blob", (0u8..=255).cycle().take(4096).collect()),
        // 略超一段（64 KiB）：测试分段查看/另存，应分 2 段
        (b"blob_70k", english_blob(70_000)),
        // 200 KB：应分 4 段；英文语料逐段不同，可直接阅读核对
        (b"blob_200k", english_blob(200_000)),
        (
            b"utf16_text",
            "UTF-16 文本"
                .encode_utf16()
                .flat_map(|u| u.to_le_bytes())
                .collect(),
        ),
        // 手动切到 gb18030 排版时，应显示简体中文（GBK 是 GB18030 子集）
        (
            b"gbk_text",
            encoding_rs::GBK.encode("GBK 简体中文").0.into_owned(),
        ),
        // 手动切到 windows-1251 排版时，应显示西里尔俄文
        (
            b"cp1251_text",
            encoding_rs::WINDOWS_1251
                .encode("Русский текст")
                .0
                .into_owned(),
        ),
        // ── 新格式：base64 / uuid / json ──
        // 手动切到 Base64 排版时，值应显示为 SGVsbG8sIE1EQlgh
        (b"base64_hello", b"Hello, MDBX!".to_vec()),
        // 手动切到 Uuid 排版时，应显示 550e8400-e29b-41d4-a716-446655440000
        (
            b"uuid_sample",
            vec![
                0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44,
                0x00, 0x00,
            ],
        ),
        // 手动切到 Json 排版时，应 pretty-print 为合法 JSON
        (
            b"json_object",
            br#"{"name":"mdbx","version":1,"tags":["fast","embedded"]}"#.to_vec(),
        ),
        (b"json_array", br#"[1,2,3,{"nested":true}]"#.to_vec()),
        // 复杂 JSON：嵌套对象/对象数组、空容器、整数/负数/小数/科学计数法、
        // bool/null、引号/反斜杠/控制符/unicode 转义、中文键值。
        // 切到 Json 排版时应 pretty-print 且按类型语法着色。
        (
            b"json_complex",
            concat!(
                r#"{"id":42,"neg":-7,"pi":3.14159,"ratio":1.5e-3,"zero":0,"#,
                r#""active":true,"disabled":false,"missing":null,"#,
                r#""tags":["db","gui","rust"],"empty_obj":{},"empty_arr":[],"#,
                r#""meta":{"version":"1.0.0","deep":[{"x":1},{"x":-2.5}],"#,
                r#""esc":"a\"b\\c\n\t\u4e2d"},"中文键":"值"}"#
            )
            .as_bytes()
            .to_vec(),
        ),
        // 非法 JSON：手动 Json 排版时应原样回退为文本
        (b"json_invalid", b"not a json string".to_vec()),
        // ── 新格式：BSON（Auto 应自动识别为 BSON，显示宽松扩展 JSON）──
        // 最小合法 BSON：空文档 {}（5 字节 "05 00 00 00 00"）
        (
            b"bson_empty",
            bson::to_vec(&bson::doc! {}).expect("valid bson"),
        ),
        // 简单文档：字符串/int/bool，与 json_object 内容对应，
        // 可对照同内容 JSON 与 BSON 两种排版的输出
        (
            b"bson_simple",
            bson::to_vec(&bson::doc! {
                "name": "mdbx",
                "version": 1,
                "active": true,
            })
            .expect("valid bson"),
        ),
        // 常见类型全覆盖：字符串/int32/int64/double/bool/null/日期/
        // ObjectId/Timestamp/二进制；datetime 与 ts_millis 同一毫秒
        // 时间戳（2025-09-28 前后），可对照 u64 时间戳排版核对
        (
            b"bson_mixed",
            bson::to_vec(&bson::doc! {
                "title": "混合类型",
                "count": 42i32,
                "big": 9_000_000_000_000i64,
                "pi": 3.14159,
                "ok": false,
                "missing": null,
                "created": bson::DateTime::from_millis(1_759_000_000_123),
                "id": bson::oid::ObjectId::from_bytes([
                    0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66,
                ]),
                "tick": bson::Timestamp {
                    time: 1_759_000_000,
                    increment: 1,
                },
                "blob": bson::Binary {
                    subtype: bson::spec::BinarySubtype::Generic,
                    bytes: vec![0xDE, 0xAD, 0xBE, 0xEF],
                },
            })
            .expect("valid bson"),
        ),
        // 嵌套结构：嵌套对象、混合类型数组、空对象/空数组、中文键值
        (
            b"bson_nested",
            bson::to_vec(&bson::doc! {
                "中文键": "值：你好，BSON！",
                "tags": ["db", "gui", 3, 2.5, true, null],
                "meta": {
                    "version": "1.0",
                    "deep": [{ "x": 1i32 }, { "x": -2.5 }],
                },
                "empty_obj": {},
                "empty_arr": [],
            })
            .expect("valid bson"),
        ),
        // ── 新格式：MessagePack / CBOR（Auto 应自动识别，显示紧凑/pretty JSON）──
        // 与 bson_simple 同内容，可对照 JSON/BSON/MSGPACK/CBOR 四种排版输出
        (
            b"msgpack_simple",
            rmpv_to_vec(&rmpv::Value::Map(vec![
                (s("name"), s("mdbx")),
                (s("version"), rmpv::Value::Integer(1.into())),
                (s("active"), rmpv::Value::Boolean(true)),
            ])),
        ),
        // 常见类型全覆盖：嵌套 map、混合数组、nil、二进制、浮点、64 位整数
        (
            b"msgpack_mixed",
            rmpv_to_vec(&rmpv::Value::Map(vec![
                (s("title"), s("混合类型")),
                (s("count"), rmpv::Value::Integer(42.into())),
                (s("big"), rmpv::Value::Integer(9_000_000_000_000i64.into())),
                (s("pi"), rmpv::Value::F64(3.14159)),
                (s("missing"), rmpv::Value::Nil),
                (
                    s("tags"),
                    rmpv::Value::Array(vec![s("db"), s("gui"), rmpv::Value::Boolean(true)]),
                ),
                (s("blob"), rmpv::Value::Binary(vec![0xDE, 0xAD, 0xBE, 0xEF])),
                (
                    s("nested"),
                    rmpv::Value::Map(vec![(s("x"), rmpv::Value::Integer((-2).into()))]),
                ),
            ])),
        ),
        // CBOR：与 msgpack 同内容，对照两种排版
        (
            b"cbor_simple",
            cbor_to_vec(&ciborium::value::Value::Map(vec![
                (txt("name"), txt("mdbx")),
                (txt("version"), ciborium::value::Value::Integer(1.into())),
                (txt("active"), ciborium::value::Value::Bool(true)),
            ])),
        ),
        (
            b"cbor_mixed",
            cbor_to_vec(&ciborium::value::Value::Map(vec![
                (txt("title"), txt("混合类型")),
                (txt("count"), ciborium::value::Value::Integer(42.into())),
                (
                    txt("big"),
                    ciborium::value::Value::Integer(9_000_000_000_000i64.into()),
                ),
                (txt("pi"), ciborium::value::Value::Float(3.14159)),
                (txt("missing"), ciborium::value::Value::Null),
                (
                    txt("tags"),
                    ciborium::value::Value::Array(vec![
                        txt("db"),
                        txt("gui"),
                        ciborium::value::Value::Bool(true),
                    ]),
                ),
                (txt("blob"), ciborium::value::Value::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF])),
                (
                    txt("nested"),
                    ciborium::value::Value::Map(vec![(
                        txt("x"),
                        ciborium::value::Value::Integer((-2).into()),
                    )]),
                ),
            ])),
        ),
        // ── 压缩容器穿透：Auto 应识别魔数并解压，显示 `gzip → JSON` 等 ──
        // 四种算法压同一份紧凑 JSON，可对照未压缩的 json_object
        (b"gz_json_object", gz(JSON_SAMPLE)),
        (b"zstd_json_object", zstd::stream::encode_all(JSON_SAMPLE, 3).unwrap()),
        (b"bz2_json_object", bz2(JSON_SAMPLE)),
        (b"xz_json_object", xz(JSON_SAMPLE)),
        // ── 地址 / FILETIME 标注：Auto 猜测行应追加解读 ──
        // 4 字节 → `IPv4 192.168.1.1`；6 字节 → `MAC 00:1A:2B:3C:4D:5E`；
        // 16 字节 → `IPv6 2001:db8::1`；8 字节 → `FILETIME → <本地时间>`
        (b"ipv4_sample", vec![192, 168, 1, 1]),
        (b"mac_sample", vec![0x00, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E]),
        (
            b"ipv6_sample",
            vec![0x20, 0x01, 0x0D, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01],
        ),
        (
            b"filetime_sample",
            ((chrono::Utc::now().timestamp() as u64 + 11_644_473_600) * 10_000_000
                + chrono::Utc::now().timestamp_subsec_nanos() as u64 / 100)
                .to_le_bytes()
                .to_vec(),
        ),
        // ── 文件魔数标注：Auto 猜测行应追加 PNG/JPEG/GIF/PDF/ZIP 标注 ──
        // 完整的 1×1 透明 PNG（RGBA），另存后可正常打开
        (b"magic_png_1x1", png_1x1()),
        // 仅头部的 JPEG（SOI + JFIF APP0 + EOI）与 GIF 逻辑屏幕头：
        // 魔数正确、无图像数据，验证标注即可
        (
            b"magic_jpeg_head",
            vec![
                0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x01, 0x00,
                0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xD9,
            ],
        ),
        (
            b"magic_gif_head",
            vec![b'G', b'F', b'I', b'8', b'9', b'a', 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00],
        ),
        (
            b"magic_pdf_head",
            concat!(
                "%PDF-1.4\n",
                "1 0 obj<</Type/Catalog>>endobj\n",
                "trailer<</Root 1 0 R>>\n",
                "%%EOF\n"
            )
            .as_bytes()
            .to_vec(),
        ),
        // 合法的空 ZIP（仅 EOCD），另存后解压工具可正常打开
        (
            b"magic_zip_empty",
            vec![
                b'P', b'K', 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
        ),
    ];
    let entries: Vec<(&[u8], Vec<u8>)> = entries.into_iter().chain(nums).collect();
    for (k, v) in entries {
        txn.put(&t, k, &v, WriteFlags::default())?;
    }
    // 超长 Key：2000 字节，接近 MDBX 4KB 页的 Key 上限（约 2022）。
    // 循环重复 "long_key_" 前缀，肉眼可辨位置；测试 Key 列截断、
    // 详情卡片/hex、收藏跳转、导出对超长 Key 的表现。
    let long_key: Vec<u8> = b"long_key_".iter().copied().cycle().take(2000).collect();
    txn.put(
        &t,
        &long_key,
        "超长的 Key（2000 字节，循环重复 long_key_）".as_bytes(),
        WriteFlags::default(),
    )?;
    // Key 和 Value 都超长：Key 1500 字节 + Value 100000 字节可读英文，
    // 测试 Key/Value 两张详情卡片同时挂大字段时的布局
    // （各自独立滚动、分段导航、字段内搜索、另存导出）。
    let long_both_key: Vec<u8> = b"keyval_long_".iter().copied().cycle().take(1500).collect();
    txn.put(&t, &long_both_key, english_blob(100_000), WriteFlags::default())?;

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
    // 一个 Key 挂 5000 个值：测试多值翻页/序号跳转/搜索回绕
    for i in 0..5_000u32 {
        txn.put(
            &t,
            b"many",
            format!("v_{i:06}").as_bytes(),
            WriteFlags::default(),
        )?;
    }
    // 注：DUPSORT 单个值大小受页大小限制（不能像普通表那样放 64KB 以上大字段），
    // 大字段分段查看用 kv_basic 里的 blob_70k / blob_200k 覆盖。

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

/// 压缩样本共用的紧凑 JSON（与 json_object 相同内容）
const JSON_SAMPLE: &[u8] = br#"{"name":"mdbx","version":1,"tags":["fast","embedded"]}"#;

/// rmpv 字符串值简写
fn s(v: &str) -> rmpv::Value {
    rmpv::Value::String(v.into())
}

/// rmpv 值序列化为 MessagePack 字节
fn rmpv_to_vec(v: &rmpv::Value) -> Vec<u8> {
    let mut buf = Vec::new();
    rmpv::encode::write_value(&mut buf, v).expect("valid msgpack");
    buf
}

/// ciborium 文本值简写
fn txt(v: &str) -> ciborium::value::Value {
    ciborium::value::Value::Text(v.to_string())
}

/// ciborium 值序列化为 CBOR 字节
fn cbor_to_vec(v: &ciborium::value::Value) -> Vec<u8> {
    let mut buf = Vec::new();
    ciborium::ser::into_writer(v, &mut buf).expect("valid cbor");
    buf
}

/// gzip 压缩
fn gz(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// bzip2 压缩
fn bz2(data: &[u8]) -> Vec<u8> {
    let mut e = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// xz 压缩
fn xz(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    lzma_rs::xz_compress(&mut std::io::Cursor::new(data), &mut out).unwrap();
    out
}

/// 生成完整的 1×1 透明 PNG（RGBA），另存后可正常打开
fn png_1x1() -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    // IHDR：宽 1、高 1、位深 8、RGBA、无压缩/滤波/隔行
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    png_chunk(&mut out, b"IHDR", &ihdr);

    // IDAT：filter 0 + 单个 RGBA 像素（透明黑），zlib 压缩
    let raw = [0u8, 0, 0, 0, 0];
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(&raw).unwrap();
    png_chunk(&mut out, b"IDAT", &e.finish().unwrap());

    png_chunk(&mut out, b"IEND", &[]);
    out
}

/// 写一个 PNG chunk：长度(4BE) + 类型 + 数据 + CRC32(类型+数据)
fn png_chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(ty);
    out.extend_from_slice(data);

    let mut crc = crc32(ty);
    crc = crc32_update(crc, data);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// PNG 用的标准 CRC32（对已初始化值继续追加数据）
fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc ^= b as u32;

        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }

    crc
}

/// PNG CRC32 入口（初值取标准 0xFFFFFFFF）
fn crc32(data: &[u8]) -> u32 {
    crc32_update(0xFFFF_FFFF, data)
}

/// 构造便于核对分段的大字段：内容为可直接阅读的英文诗与散文
/// （均为公有领域作品），语料循环时插入 `--- pass N ---` 分隔，
/// 使每个 64 KiB 段读起来都不一样；每 256 字节块开头另带
/// ASCII 绝对偏移标记 `<<< @XXXXXXXX >>>`，可对照 hex 地址列。
fn english_blob(len: usize) -> Vec<u8> {
    let corpus = ENGLISH_TEXT.as_bytes();
    let mut out = Vec::with_capacity(len);
    let mut pending: std::collections::VecDeque<u8> = std::collections::VecDeque::new();
    let mut ci = 0usize;
    let mut pass = 1usize;
    while out.len() < len {
        // 绝对偏移标记：严格落在 256 字节边界
        if out.len() % 256 == 0 {
            pending.extend(format!("<<< @{:08X} >>>\n", out.len()).bytes());
        }
        let b = if let Some(b) = pending.pop_front() {
            b
        } else if ci < corpus.len() {
            let b = corpus[ci];
            ci += 1;
            b
        } else {
            // 语料走完一轮：插入轮次分隔后从头再循环，
            // 保证不同段的可读内容明显不同
            pending.extend(format!("\n--- pass {pass} ---\n").bytes());
            pass += 1;
            ci = 0;
            pending.pop_front().unwrap()
        };
        out.push(b);
    }
    out
}

/// 大字段语料：英文诗与散文（公有领域）。
const ENGLISH_TEXT: &str = r#"
The Road Not Taken
By Robert Frost

Two roads diverged in a yellow wood,
And sorry I could not travel both
And be one traveler, long I stood
And looked down one as far as I could
To where it bent in the undergrowth;

Then took the other, as just as fair,
And having perhaps the better claim,
Because it was grassy and wanted wear;
Though as for that the passing there
Had worn them really about the same,

And both that morning equally lay
In leaves no step had trodden black.
Oh, I kept the first for another day!
Yet knowing how way leads on to way,
I doubted if I should ever come back.

I shall be telling this with a sigh
Somewhere ages and ages hence:
Two roads diverged in a wood, and I -
I took the one less traveled by,
And that has made all the difference.

Ozymandias
By Percy Bysshe Shelley

I met a traveller from an antique land,
Who said - "Two vast and trunkless legs of stone
Stand in the desert. . . . Near them, on the sand,
Half sunk a shattered visage lies, whose frown,
And wrinkled lip, and sneer of cold command,
Tell that its sculptor well those passions read
Which yet survive, stamped on these lifeless things,
The hand that mocked them, and the heart that fed;

And on the pedestal, these words appear:
My name is Ozymandias, King of Kings;
Look on my Works, ye Mighty, and despair!
Nothing beside remains. Round the decay
Of that colossal Wreck, boundless and bare
The lone and level sands stretch far away.

Sonnet 18
By William Shakespeare

Shall I compare thee to a summer's day?
Thou art more lovely and more temperate:
Rough winds do shake the darling buds of May,
And summer's lease hath all too short a date;
Sometime too hot the eye of heaven shines,
And often is his gold complexion dimm'd;
And every fair from fair sometime declines,
By chance or nature's changing course untrimm'd;
But thy eternal summer shall not fade,
Nor lose possession of that fair thou ow'st;
Nor shall death brag thou wander'st in his shade,
When in eternal lines to time thou grow'st:
So long as men can breathe or eyes can see,
So long lives this, and this gives life to thee.

Sonnet 116
By William Shakespeare

Let me not to the marriage of true minds
Admit impediments. Love is not love
Which alters when it alteration finds,
Or bends with the remover to remove:
O no; it is an ever-fixed mark,
That looks on the tempest, and is never shaken;
It is the star to every wandering bark,
Whose worth's unknown, although his height be taken.
Love's not Time's fool, though rosy lips and cheeks
Within his bending sickle's compass come:
Love alters not with his brief hours and weeks,
But bears it out even to the edge of doom.
If this be error and upon me proved,
I never writ, nor no man ever loved.

I Wandered Lonely as a Cloud
By William Wordsworth

I wandered lonely as a cloud
That floats on high o'er vales and hills,
When all at once I saw a crowd,
A host, of golden daffodils;
Beside the lake, beneath the trees,
Fluttering and dancing in the breeze.

Continuous as the stars that shine
And twinkle on the milky way,
They stretched in never-ending line
Along the margin of a bay:
Ten thousand saw I at a glance,
Tossing their heads in sprightly dance.

The waves beside them danced; but they
Out-did the sparkling waves in glee:
A poet could not but be gay,
In such a jocund company:
I gazed - and gazed - but little thought
What wealth the show to me had brought:

For oft, when on my couch I lie
In vacant or in pensive mood,
They flash upon that inward eye
Which is the bliss of solitude;
And then my heart with pleasure fills,
And dances with the daffodils.

The Tyger
By William Blake

Tyger Tyger, burning bright,
In the forests of the night;
What immortal hand or eye,
Could frame thy fearful symmetry?

In what distant deeps or skies.
Burnt the fire of thine eyes?
On what wings dare he aspire?
What the hand, dare seize the fire?

And what shoulder, & what art,
Could twist the sinews of thy heart?
And when thy heart began to beat,
What dread hand? & what dread feet?

What the hammer? what the chain,
In what furnace was thy brain?
What the anvil? what dread grasp,
Dare its deadly terrors clasp!

When the stars threw down their spears
And water'd heaven with their tears:
Did he smile his work to see?
Did he who made the Lamb make thee?

Tyger Tyger burning bright,
In the forests of the night:
What immortal hand or eye,
Dare frame thy fearful symmetry?

An Essay on Friendship and Labour

When we examine the records of an old library, we find that
books are patient companions. They wait on the shelf through long
winters and quiet summers, asking nothing of us but attention.
A reader who returns after years of absence may open the same
volume and find the same words, placed as carefully as the day
they were printed. Memory is a page index: it tells us where we
have been, and patience turns its leaves without haste.

Consider how a craftsman learns a trade. The first work is
clumsy, the second is careful, the third begins to look easy,
and only the fortieth is truly simple. Skill is not a sudden
gift but the slow deposit of repeated mornings. Every careful
stroke leaves a little of itself in the hand, every mistake
teaches the next attempt what it must avoid.

Travel teaches a similar lesson. A road walked once is only
a story; walked many times, it becomes a habit and then a home.
The distant mountain does not move toward the traveller; the
traveller moves toward the mountain one small step at a time,
and the journey itself, honestly undertaken, is most of what
the summit gives. The view at the top is often plainer than
the long climb in memory.

So keep good company: honest books, honest tools, honest
friends. A well made table rewards the elbow; a well made
sentence rewards the eye; a well kept promise rewards the
years. Whatever is worth doing at all is worth doing more
than once, and the second doing is where mastery quietly begins.
"#;
