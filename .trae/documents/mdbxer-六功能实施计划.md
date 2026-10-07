# MDBXer 六功能实施计划

## Context

MDBXer 目前是纯浏览型只读查看器：有翻页/排序/多值导航/大字段分段/格式解码/i18n，但缺少查找定位、数据导出、个性化三项能力。用户确认新增 6 项功能（多值表值总数列已存在，从清单剔除）：

1. 主表 Key 搜索（跳转 + 前缀过滤）
2. 导出 CSV/JSON（后台线程 + 进度）
3. 更多解码格式（Base64 / UUID / JSON）
4. 深浅色主题切换
5. 状态记忆（UI 偏好 + 每库最后表）
6. 收藏夹（表级星标置顶 + Key 级跳转）

关键已验证事实：
- `db::jump_to`（set_lowerbound 定位）已存在（[page.rs](file:///k:/mdbx/mdbxer/src/db/page.rs#L186-L232)），跳转功能只需接 UI
- `parse_bytes_input`（文本 / hex(...) / 0x... 解析）已存在（[mod.rs](file:///k:/mdbx/mdbxer/src/ui/mod.rs#L771)），搜索输入直接复用
- `parse_jump_input`（[mod.rs](file:///k:/mdbx/mdbxer/src/ui/mod.rs#L749)）已支持 INTEGER_KEY 表十进制输入
- `Database<NoWriteMap>` 是 Send+Sync，但只读事务生命周期绑定 &Database，不能进 `thread::spawn`；**导出线程内重新 `DbHandle::open`**（只读+ACCEDE 零开销，且用户中途关库不影响导出）
- `serde_json`、`rfd` 已是依赖；base64 手实现 RFC 4648（约 30 行），不引新 crate
- 多值表 Key 行已显示 `〔N 个值〕`（[dataview.rs](file:///k:/mdbx/mdbxer/src/ui/dataview.rs#L220-L250)）

## 分批顺序

| 批 | 内容 | 依赖 |
|---|---|---|
| 1 | 解码格式 Base64/Uuid/Json | 无（独立纯函数） |
| 2 | Key 搜索（跳转+前缀过滤） | db 层 fetch_page 加前缀约束 |
| 3 | 导出 CSV/JSON | 批1解码器、后台线程模型 |
| 4 | 主题+状态记忆+收藏夹 | 三者共用 config.rs 一次扩展 |

每批：`cargo check` 零警告 → `cargo test` 全过 → 中文 commit。

## 批 1：解码格式（fmt 层）

**改 `src/fmt/mod.rs`**：`DecodeMode` 加 `Base64/Uuid/Json` 三变体；`ALL` 数组 17→20；`label()` 返回 `"base64"/"uuid"/"json"`（技术名不翻译，同 int8/hex，不加 i18n key）；新增 `as_str()/from_str()`（稳定字符串如 `"b64"/"uuid"/"json"`，供批4配置序列化用，未知回退 Auto）。

**改 `src/fmt/value.rs`** `decode()` 加三分支（纯函数）：
- Base64：手写 RFC 4648 编码（字节→base64 文本）
- Uuid：仅 `len==16` 时 `8-4-4-4-12` 小写 hex；其他长度回退 hex_spaced
- Json：`serde_json::from_slice` 成功 → `to_string_pretty`；失败回退 `from_utf8_lossy`
- 三者遵守空输入返回空串约定

**`src/fmt/guess.rs` 不动**（Auto 嗅探保持不变，避免误判）。

**测试**（value.rs tests mod）：base64 基本/空/非对齐余数；uuid 16 字节标准向量；uuid 非 16 字节回退；json pretty/非法回退；ALL 长度 20；as_str/from_str 往返。

## 批 2：Key 搜索（跳转 + 前缀过滤）

**改 `src/db/page.rs`**：新增 `fetch_page_prefix(db, table, dup_sort, dir, prefix: Option<&[u8]>, anchor, skip_anchor, limit)`；`fetch_page` 改为调它并传 `None`（签名不变）。前缀约束进 `collect`：每条先查 `key.starts_with(prefix)`，不匹配即 break 且 `has_more=false`；升序起点 `set_lowerbound(prefix)`，降序起点用 `set_upperbound(prefix ++ [0xFF;N])`（N 取 64）落到前缀末位。`src/db/mod.rs` re-export。

**改 `src/ui/mod.rs`** `MdbxerApp` 加字段：`key_search_input: String`、`key_filter: Option<Vec<u8>>`、`key_filter_mode: bool`（false=跳转）。方法：
- `apply_key_search()`：跳转模式复用现有 jump_to 逻辑并选中首行；前缀模式存 `key_filter` + 重新加载首页
- `clear_key_filter()`：清过滤 + 重载
- 内部 fetch 改调 `fetch_page_prefix`，传 `key_filter.as_deref()`

**改 `src/ui/dataview.rs`** 控制区（跳页框前）：`TextEdit::singleline` 宽 140、hint 三语；`→`/`⊂` 单字按钮切模式（hover 说明）；回车触发；过滤激活时显示 `×` 清除按钮 + 「已过滤」提示。保持单行紧凑约束。

**i18n 新 key**：`key_search_hint`、`key_search_mode_tip`、`filter_active`、`filter_clear_tip`、`key_search_bad(e)`（方法+fill）。

**风险**：INTEGER_KEY 表前缀过滤按字节语义（文档注明）；页内排序与过滤正交无冲突。

**测试**：i18n 三语新 key 非空；starts_with 判定纯逻辑。

## 批 3：导出 CSV/JSON（后台线程）

**新增 `src/export.rs`**（SPDX 头）：
```rust
pub enum ExportFormat { Csv, Json }
pub struct ExportJob { db_path, open_mode, table, dup_sort, integer_key,
                       sort_desc, key_mode, val_mode, endian, out_path, format } // 全 Send
pub enum ExportProgress { Progress(usize), Done(usize), Fail(String) }
pub fn start(job: ExportJob) -> mpsc::Receiver<ExportProgress>
```
线程主体：重新 `DbHandle::open` → `BufWriter::new(File::create(..))` → 只读事务 cursor 遍历（升序 iter_start；降序 last+prev 循环，多值表用逐条 prev 而非 prev_nodup）。CSV：header `key,value`，多值表每值一行，本地实现转义（含 `,"`/`\n` 加引号双写）；JSON：普通表 `[{key,value}]`，多值表流式写 `{key:[values]}`。解码用 `fmt::decode(.., 不截断)`。每 1000 条发 Progress；结束发 Done/Fail。

**改 `src/ui/mod.rs`**：字段 `export_rx: Option<Receiver<..>>`、`export_count: usize`、`export_format: ExportFormat`；方法 `start_export()`（rfd 选路径、防重入）+ `poll_export(ctx)`（每帧 try_recv，更新状态栏，进行中 `ctx.request_repaint()`）。

**改 `src/ui/dataview.rs`** 控制区末尾：CSV/JSON 下拉（.width 60）+ `⭳` 按钮（导出中禁用）。

**i18n 新 key**：`export_tip`、`export_started`、`export_progress(n)`、`export_done(n,path)`、`export_failed(e)`。

**风险**：中途关库不受影响（独立 env）；流式写无整表物化；integer_key 表导出解码与显示一致（不特化）。

**测试**：CSV 转义纯函数（逗号/引号/换行）；JSON 拼接纯函数。

## 批 4：主题 + 状态记忆 + 收藏夹

**改 `src/config.rs`**（一次扩展，全部 `#[serde(default)]` 向后兼容）：
```rust
ConfigFile { lang, theme: Option<String>, ui: UiPrefs, per_db: Vec<PerDbRecord> }
UiPrefs { page_size, endian_le, key_mode, val_mode, thousands_sep }  // 全 Option
PerDbRecord { path, last_table, fav_tables: Vec<Option<String>>,
              fav_keys: Vec<FavKey>, last_use }  // LRU 上限 50
FavKey { table, key_hex, note }
```
API：`load_prefs()`、`save_theme()`、`save_ui_prefs()`、`load_per_db(path)`（touch LRU）、`save_per_db(rec)`（upsert+截断）；统一「读-改-写」`write_config()`。新增 `Theme { Dark, Light }` 枚举。

**主题**：`main.rs` 启动时 `cc.egui_ctx.set_visuals()`；`topbar.rs` 语言下拉左加 `☀`/`🌙` 按钮 → `app.toggle_theme(ctx)`（set_visuals + save_theme）。

**状态记忆**：`MdbxerApp::new` 应用 UiPrefs（page_size 校验合法值、endian、key/val mode from_str、thousands_sep）；各下拉/开关变更点调 `save_ui_prefs(&app.current_ui_prefs())`；`open_db` 成功后按 `last_table` 自动选表（找不到回退主表）；切表/关库时 `save_per_db`。

**收藏夹**：
- `src/ui/sidebar.rs` 表名行内加 `★/☆` 小按钮切换；排序后稳定 partition 收藏表前置；底部 `CollapsingHeader` 收藏 Key 列表（表名+key 摘要+跳转+×）
- `src/ui/detail.rs` Key 卡片标题行加 `☆/★` 按钮 → `toggle_fav_key(key_bytes)`
- `src/ui/mod.rs`：`fav_tables`/`fav_keys` 字段（open_db 时加载）；`toggle_fav_table/toggle_fav_key`（查删/查增+save_per_db）；`jump_to_fav(fk)` = select_table + 复用批2 apply_key_search 跳转

**i18n 新 key**：`theme_tip`、`favorites_title`、`fav_table_tip`、`fav_key_tip`、`fav_jump_tip`、`fav_del_tip`、`fav_empty`。

**测试**：ConfigFile 旧格式（仅 lang）解析兼容；UiPrefs 序列化往返；LRU 截断 50；DecodeMode as_str/from_str 20 变体往返。

## 涉及文件汇总

- `src/fmt/mod.rs`、`src/fmt/value.rs`（批1）
- `src/db/page.rs`、`src/db/mod.rs`、`src/ui/dataview.rs`（批2、批3 UI）
- `src/export.rs`（新增，批3）
- `src/config.rs`、`src/main.rs`、`src/ui/topbar.rs`、`src/ui/sidebar.rs`、`src/ui/detail.rs`（批4）
- `src/ui/mod.rs`（批2/3/4 状态与动作）
- `src/i18n.rs`（每批三语新 key）

## 验证

每批：`cargo check` 零警告 → `cargo test` 全过 → 中文 commit。手动验证：
- 批1：排版下拉 20 项；base64/uuid/json 解码正确，非 16 字节 uuid 回退 hex
- 批2：文本/hex 跳转、前缀过滤翻页（升降序、首末页）、×清除恢复、INTEGER_KEY 表数字跳转
- 批3：小表 CSV/JSON 内容正确（多值表 CSV 每值一行、JSON `{k:[v]}`）；大表进度不卡 UI；中途关库导出仍完成；无权限目录报错
- 批4：主题即切即存；UI 偏好重启保留；每库 last_table 恢复（表被删回退主表）；表星标置顶、Key 收藏跳转、重启保留；三语文案齐全

全部完成后更新 `docs/功能点与知识点.md`（顶部日期 + 解码格式章节 + 新章节：Key 搜索与前缀过滤、导出、主题、状态记忆、收藏夹；知识点：mdbx 跨线程模型、降序前缀 upperbound 构造、serde default 平滑扩展）。
