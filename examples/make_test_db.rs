// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

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
        (b"i64_9000000000000_neg", (-9_000_000_000_000i64).to_le_bytes().to_vec()),
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
        (b"f64_1.41421356_neg", (-1.41421356f64).to_le_bytes().to_vec()),
        (b"f64_3.14159265", 3.14159265f64.to_le_bytes().to_vec()),
        (b"f64_1e100", 1e100f64.to_le_bytes().to_vec()),
        // 整数部分长：应显示 123,456,789.25 / -9,876,543,210.5
        (b"f64_123456789.25", 123456789.25f64.to_le_bytes().to_vec()),
        (b"f64_9876543210.5_neg", (-9876543210.5f64).to_le_bytes().to_vec()),
    ];
    let entries: Vec<(&[u8], Vec<u8>)> = vec![
        (b"hello", b"world".to_vec()),
        ("中文键".as_bytes(), "中文值：你好，MDBX！".as_bytes().to_vec()),
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
    ];
    let entries: Vec<(&[u8], Vec<u8>)> = entries
        .into_iter()
        .chain(nums)
        .collect();
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
