# MDBXer — libmdbx 数据库查看工具实现计划

## 概述

使用 Rust + egui 0.36.2 (eframe/egui_extras) + libmdbx-rs master 编写一个 libmdbx 数据库**纯只读**查看工具（GUI 风格参考用户提供的 MDBX Ray 截图：左侧表列表、中间 数据/表统计/环境信息、右侧 KV 详情）。

工作区 `k:\mdbx\mdbxer` 已有骨架：`Cargo.toml`（eframe 0.36.2、egui_extras 0.36.2、`libmdbx = { git = ".../libmdbx-rs.git", branch = "master" }`，依赖已成功 resolve 并 cargo check 通过）+ 空 `src/main.rs`。

### 已验证的 libmdbx-rs master API（commit aba108e，crate 名 `libmdbx` v0.9.0）

- 打开：`libmdbx::Database::<NoWriteMap>::open_with_options(path, DatabaseOptions { max_tables: Some(4096), no_sub_dir, accede: true, mode: Mode::ReadOnly, ..Default::default() })`
- `db.begin_ro_txn()` → `txn.open_table(Option<&str>)`（None=主表）、`txn.table_stat(&t) -> Stat`（`entries()/depth()/branch_pages()/leaf_pages()/overflow_pages()/page_size()/total_size()`）、`txn.table_flags(&t) -> TableFlags`（`DUP_SORT/INTEGER_KEY/DUP_FIXED/...`）、`txn.cursor(&t)`
- Cursor：`first()/next()/prev()/last()/get_current()/set_range(key)/set_lowerbound(key, Option<val>)/iter_start()/iter_from(key)/into_iter_back_start()`，泛型 `Decodable`，`Vec<u8>` 已实现
- `db.info() -> Info`（`map_size()/last_pgno()/last_txnid()/max_readers()/num_readers()/geometry()→min/max/current/grow/shrink`）、`db.stat()`、`db.freelist()`
- **没有** list_tables API → 枚举 subDB：遍历主表 key，逐个 `open_table(Some(name))`（无 CREATE 标志，非表 key 返回 NotFound 则跳过）；若一个命名表都没有且主表有数据，则显示"（主表）"伪条目
- 多值表定位用 `set_lowerbound(key, Some(value))` 可精确到 (key,value) 对，分页锚点基于此

## 分层架构（代码隔离）

严格单向依赖：`main → ui → db / fmt / history`。下层绝不引用上层，egui 类型只出现在 `ui` 与 `main` 中，libmdbx 类型只出现在 `db` 中（ui 只见自有纯数据结构），`fmt` 为纯函数库。

```
src/
├── main.rs              入口：eframe 启动、视口配置、CJK 字体加载、组装 App（不含业务逻辑）
├── db/                  【DB 层】仅依赖 libmdbx + std，不依赖 egui/fmt/ui
│   ├── mod.rs           re-export + DbError(String 化错误) + OpenMode{自动/单文件/目录}
│   ├── handle.rs        DbHandle::open(path, OpenMode)（自动识别 no_sub_dir、accede、只读）；
│   │                    枚举 subDB（遍历主表 key 逐个 open_table 试探）；TableInfo{name, entries,
│   │                    flags_desc, dup_sort, integer_key}；无主表数据时给"（主表）"伪条目
│   ├── page.rs          fetch_page(表, 方向, 锚点 Option<(key, Option<val>)>, 页大小) -> Page{rows, 到端点}；
│   │                    jump_to(表, key字节, 页大小)；dups_of(表, key, dup页码, 页大小) -> (dup总数, 当前页值)
│   └── stats.rs         table_stat_view(表) -> Vec<(标签, 值)>；env_info_view() -> Vec<(分组, 标签, 值)>
│                        （Info+Stat+freelist 格式化为纯键值行，UI 直接渲染）
├── fmt/                 【格式解析层】纯函数，仅依赖 std + chrono，不依赖 libmdbx/egui
│   ├── mod.rs           DecodeMode{自动/I8/I16/I32/I64/U8/U16/U32/U64/F32/F64/UTF8/UTF16LE/UTF16BE/Hex/Dec/Binary} + 标签
│   ├── guess.rs         guess(bytes) -> (类型标签, 文本)：空→"空"；可打印 UTF-8→文本；
│   │                    len 8→u64 LE（落 epoch 范围附本地日期）；len 4→u32；len 2→u16；其余→Hex
│   ├── value.rs         decode(bytes, mode, 单元格最大长度) -> String（截断加省略号）；epoch→本地时间(chrono)
│   └── hexdump.rs       hex_dump(bytes, 显示地址, 显示HEX, 显示ASCII) -> String（16 字节/行）
├── history.rs           【持久层】仅依赖 serde/serde_json + std：load/save/add/remove，
│                        路径 %APPDATA%/mdbxer/history.json（写失败静默降级为会话内存）
└── ui/                  【界面层】依赖 egui/eframe/egui_extras/rfd + db + fmt + history
    ├── mod.rs           MdbxerApp（eframe::App）：持有 DbHandle 与纯数据状态（rows、选中行、
    │                    页码、排序、DecodeMode 等），定义导航动作（打开/选表/翻页/首末条/跳转/选行），
    │                    拖放文件打开、↑/↓ 键移动选中
    ├── topbar.rs        顶栏：路径输入、打开、浏览文件/目录(rfd)、模式下拉、历史下拉、
    │                    排版(Value 列格式)、单元格长度(默认 256)、表/属性按钮
    ├── sidebar.rs       左侧表列表：过滤框、排序下拉(名称/条数)、名称+N条+flags 角标、选中高亮
    ├── dataview.rs      中间"数据"页签：工具条(排序升降/⏮◀▶⏭/跳转输入/每页条数 50·100·200·500·1000
    │                    默认 200/"第 a~b 条 / 共 N 条") + egui_extras::TableBuilder 四列 #/Key/类型/Value，
    │                    行点击选中，Value 按排版解码并截断
    ├── statsview.rs     "表统计"与"环境信息"页签：渲染 db::stats 返回的键值行（两栏网格）
    └── detail.rs        右侧详情：Key 卡片(Key #n、格式下拉、复制、文本、hex dump) + Value 卡片
                         (dup "第 x/y 个值" + ◀▶、格式下拉、复制、文本、hex dump) +
                         三个开关 checkbox：显示地址 / HEX / ASCII
```

- `Cargo.toml`（修改）：加 `chrono`(default-features=false, features=["std","clock"])、`serde`/`serde_json`、`rfd`

层间契约：`ui` 从 `db` 只拿到 `TableInfo`、`Row{key: Vec<u8>, value: Vec<u8>}`、`Vec<(标签,值)>` 等自有类型；事务在 db 层函数内部开闭；`fmt` 输入输出均为 `&[u8]`/`String`。

## 需求拆解与决策

| # | 需求 | 实现决策 |
|---|------|---------|
| 1 | 两种模式自动识别 | 路径为文件 → `no_sub_dir=true`（单文件）；为目录 → 子目录模式。"自动/单文件/目录"下拉可手动覆盖 |
| 2 | 历史记录 | JSON 存 `%APPDATA%/mdbxer/history.json`（失败则退回 exe 同目录），顶栏"历史"下拉，记录 path+mode+时间，点击直接打开，可删除单条 |
| 3 | 多值 (DUPSORT) | 数据页把 dup 展平为独立行（`cursor.next()` 天然如此）；右侧详情显示该 key 的全部 dup 并分页（`iter_dup_of`） |
| 4 | 格式猜测+手选 | `DecodeMode`：自动/i8/i16/i32/i64/u8/u16/u32/u64/f32/f64/UTF-8/UTF-16LE/UTF-16BE/Hex/Dec/Binary。猜测顺序：空→"空"；可打印 UTF-8→文本；len 8→u64 LE（落 epoch 范围附日期）；len 4→u32 LE；len 2→u16 LE；其余→Hex |
| 5 | 超大表分页 | 每次只取一页（`set_lowerbound` 锚点 + first/next/prev/last，O(页大小)），每页条数下拉 50/100/200/500/1000，默认 200；详情里 dup 也分页 |
| 6 | 左表列表 | 名称+N条+flags 角标，排序下拉（名称/条数，升/降），顶部过滤输入框 |
| 7 | 中间三页签 | 数据 / 表统计（Stat+flags）/ 环境信息（Info+Stat+freelist） |
| 8 | 右侧详情 | 选中行后 Key/Value 各自：格式下拉（自动=显示猜测类型徽标）、复制按钮、解码文本框、hex dump；Value 显示该 key dup 总数与 dup 翻页 |
| 9 | 详情开关 | 三个 checkbox：显示地址（offset 列）/ HEX / ASCII，控制 hex dump 组成 |
| 10 | 搜索定位 | 跳转式：输入 `hex(0a3f21)`/`0x..`/纯文本；INTEGER_KEY 表另接受十进制（自动试 4/8 字节 LE）。`set_range` 定位后以该 key 为页首刷新 |

其余决策：只读打开（`accede:true`，不创建锁文件冲突，可查看被占用的库）；CJK 字体运行时加载 `C:\Windows\Fonts\msyh.ttc`（含 simhei/simsun/PingFang/Noto 回退）；支持拖拽文件/目录到窗口直接打开；时间戳注释用 chrono（`default-features=false, features=["std","clock"]`）格式化本地时间。

### 分页/导航正确性要点（db/page.rs 实现）

- 锚点 = `(key, Option<value>)`，value 仅 DUP_SORT 表传入；定位用 `set_lowerbound`（存在则精确，否则落在下一对，可接受）
- "下一页"：锚点=当前页末行，排他（定位后先 next/prev 一步再取）
- "上一页"：锚点=当前页首行，反方向排他取一页后 reverse
- "末条"：从尾部反向取一页后 reverse（asc 时）；行号用估算偏移（首页 0 起、末页 total-len 起、跳转型导航后显示相对序号）

## 假设

- 纯查看工具，不提供任何写入；始终以只读+accede 打开
- 主表 key 必须是合法 UTF-8 才会被尝试当作命名表名（非 UTF-8 主表数据不会被误列）
- 历史记录失败（无权限写盘）时静默降级为仅本次会话内存记录

## 验证步骤

1. `cargo check` 至零错误
2. 新增 `examples/make_test_db.rs`：生成含普通 subDB、DUPSORT 多值表、INTEGER_KEY 表、大表（≥1万条）、中文 value、空 value、u64 时间戳 value 的测试库（目录模式与单文件模式各一）
3. `cargo run --example make_test_db` 生成后，`cargo build --release` 构建主程序
4. 手动/截图验证：打开两种模式库、表列表计数、分页翻页/首末条、跳转（文本/hex/整数 key）、多值详情翻页、格式自动猜测与手选、HEX/ASCII/地址开关、历史记录重开
