# egui / eframe 知识点（0.36.2）

> 本应用的桌面 UI 基于 `eframe` + `egui`（含 `egui_extras`）0.36.2。
> 本文集中记录 egui 的**底层机制、常用 API、本项目踩过的坑与工程实践**，
> 以源码阅读得出的结论为主，避免凭印象推断。

**维护规则**：每当对话中讨论到本文尚未收录的 egui 知识点，应在回答后
**自动追加 / 更新本文档**（写入对应小节，必要时新增小节），无需用户再次
提醒；同时更新顶部的"最后更新"日期。

最后更新：2026-10-11（新增：context.rs 功能总表；一帧的完整旅程）

---

## 1. 架构核心：`Ui` 是什么

`Ui` 是 egui 里放置控件的"画布/区域"——代表屏幕上一块带**布局方向**的矩形。每个 Panel / Window / Area / `horizontal`/`vertical` 容器内部都会创建一个 `Ui`。

### 1.1 `Ui` 的关键字段

- `id`：**稳定 ID**（父 id + salt 派生），跨帧不变，用于存持久化状态（窗口位置、折叠面板是否展开）。
- `unique_id`：**全局唯一但不稳定**，基于控件在层级树中的位置；只用于当帧交互，不能存状态。
- `painter`：实际绘制对象（图层 + 裁剪矩形 + `Context`），`ui.painter().text/galley` 手动画字。
- `style: Arc<Style>`：视觉样式，**多个 Ui 共享**，`style_mut()` 走 **copy-on-write**。
- `placer: Placer`：布局引擎，管尺寸和"新控件摆在哪"。
- `stack: Arc<UiStack>`：层级栈信息（父节点、布局方向、min/max rect）。
- `sense: Sense`：Ui 背景的交互感知（点击/拖拽/可聚焦/悬停）。

### 1.2 `Ui` 自动解引用成 `Context`

```rust
impl Deref for Ui { type Target = Context; ... }
```

所以 `ui.input(...)`、`ui.memory(...)`、`ui.request_repaint()` 这些其实属于 `Context` 的方法能直接调。只实现了 `Deref`，**没有 `DerefMut`**——经 Ui 只能拿到 `&Context`，全局修改走 Context 提供的 `&self` 内部可变 API。

### 1.3 `Ui::new` vs `Ui::new_child`

| | `Ui::new`（顶层） | `new_child`（嵌套） |
|---|---|---|
| 用途 | Panel/Window/Area | 任意嵌套子区域 |
| id | 外部给定，无 parent | 由父 id 派生，挂 parent |
| style/painter | 从 Context 取 | 从父 Ui 继承（COW） |

`new_child` 的关键：默认在父 Ui 的剩余可用空间里造子 Ui，有独立的 `Placer`，但通过 `stack.parent` 挂回父节点。**`new_child` 不自动占父空间**——闭包跑完后要把子 Ui 的 `min_rect` 报备给父布局，否则下一个控件会重叠。`scope`/`horizontal`/`vertical`/`表格 col` 内部封装了这一步。

---

## 2. 布局：控件为什么"不填满"

### 2.1 Label 永远只占内容大小

`Label::ui` 底层调用 `layout_in_ui` → `ui.allocate_exact_size(galley.size(), sense)`：

- **宽**：不换行时 = 最长行宽；换行时 = 换行边界宽；
- **高** = `行数 × 行高`（空字符串也至少一行）。

**Label 没有"填满可用宽度/高度"的能力**。要让某控件占满整列/整行：
- `ui.allocate_rect(ui.available_rect_before_wrap(), sense)` + 手动画字；或
- `ui.add_sized([w, h], widget)` / `ui.put(rect, widget)`；或
- 父 Layout 用 `justify`（主轴拉伸）。

### 2.2 `allocate_space` / `allocate_rect` / `interact`

- `allocate_space(size) -> (Id, Rect)`：**只占空间**，向 Placer 申请下一块至少 `size` 的矩形并推进光标；不绘制、不交互。最底层。
- `allocate_rect(rect, sense) -> Response`：占一块**指定矩形**并直接带上交互。
- `interact(rect, id, sense) -> Response`：不占位，只让已存在的 rect 可点/可拖。

### 2.3 `ui.add(widget)` 实际在做什么

```rust
pub fn add(&mut self, widget: impl Widget) -> Response { widget.ui(self) }
```

`add` **不负责"添加"**——它把 `&mut Ui` 交给控件，**控件在自己的 `Widget::ui` 实现里**完成占位、绘制、交互注册。这是即时模式的本质：没有持久的控件树，"添加"就是当场画上去。

### 2.4 `Sense` 的 HOVER 是 0

```rust
const HOVER = 0;      // 不是一个真正的位，而是"无任何交互位"的空状态
const CLICK = 1 << 0;
const DRAG  = 1 << 1;
```

一个控件**永远能 hover**；click/drag/focusable 才是需要置 1 的额外能力。所以 `Sense::hover()` = `empty()`。

### 2.5 表格 body 整行可点的机制

`egui_extras::TableBuilder.sense(Sense::click())` 会把 sense 传给每个 cell 的 child Ui（背景 sense），再经 `set_min_size(max_rect.size())` 把交互区撑到整格，各列 `.union()` 合并成整行。所以：

- **单元格内的 Label 不应加 click sense**——可交互 Label 在 hit-test 中位于 cell interact 之上，会抢走 hover（"悬停文字无高亮、悬停空白才有"）。
- 点击统一交给 `row_ui.response()`。
- **表头是特例**：egui_extras 没替表头挂"整格点击"，需自己实现（Label 画字 + `allocate_rect` 盖空白，或整块 `interact` + 手动画字）。

### 2.6 滚动的核心：`scroll_with_delta`

```rust
pub fn scroll_with_delta(&mut self, delta: Vec2) {
    if delta == Vec2::ZERO { return; }
    let layer_rect = self.clip_rect_stack.last().cloned()?;
    let mut visible = self.ctx.memory(|m| m.area_visible_rect(&layer_rect));
    visible.translate(-delta);                     // ← 反向平移
    self.ctx.memory_mut(|m| m.set_area_visible_rect(layer_rect, visible));
    self.ctx.request_repaint();
}
```

**egui 滚动的真相**：滚动位置不存控件里，而存在 `Context` 的 **"区域可见矩形"**（`area_visible_rect`，key 是当前 Ui 的 `layer_rect`）。"滚动" = 把这块可见区 `Rect` 反向平移后写回 memory；`ScrollArea` 每帧按此 `visible_rect` 决定绘制哪段。

- 上层 API 都落在此：`scroll_to_rect(rect, align)` 算 delta 后调它；`scroll_to_cursor_animation` 在其上加缓动；表格 `scroll_to_row`、hex 视图边缘自动滚动也都归结为改这块 Rect。
- **delta 方向与直觉相反**（用 `-delta`）：想让内容"向上走"，`delta.y -= 64.0`；压上边缘应传正、下边缘传负。
- 注意：`ScrollArea` 首次需要 sizing pass 才能决定滚动条 thumb 长度（内容总高度未知），这部分在 `ScrollArea` 内部，不在 `Ui`。

---

## 3. Style 的 copy-on-write

```rust
pub fn style_mut(&mut self) -> &mut Style {
    Arc::make_mut(&mut self.style)   // clone-on-write
}
```

`Arc::make_mut(&mut arc)`：当引用计数 > 1 时，clone 出独立副本并把 `self.style` 指向新副本（`*arc = Arc::new(...)`），再返回其可变引用。

- **"没真改也会复制"**：只看引用计数，不看是否真的写入。所以读样式用 `style()`，要改才 `style_mut()`。
- **适合小数据**：复制粒度是整个 `Style`，`Style` 字段都是小结构所以便宜。大数据不该用整包 `Arc` COW。
- **作用域**：只影响当前 Ui 及其**之后创建的子 Ui**（子 Ui 会 `Arc::clone` 走这份新 style），改之前已创建的兄弟/父 Ui 不受影响。`scope` 是隔离样式改动的标准手法。

---

## 4. 文本与排版

### 4.1 `RichText` vs `WidgetText` vs `TextStyle`

- `RichText`：带样式的文本（加粗、颜色、字号、字体），是 `WidgetText::RichText` 变体。
- `TextStyle`：语义化名字（Small/Body/Button/Heading/Monospace…），通过 `Style::text_styles` 查表得到 `FontId`。
  - `Monospace` 名字有歧义：等宽本质是 `FontFamily::Monospace`（字体家族）的特性，`TextStyle::Monospace` 只是个"正文大小 + 等宽家族"的便捷别名。
- `WidgetText` 三种变体：`Text` / `RichText` / `Galley`（已排版好的）。

### 4.2 自定义 layouter 与多色文本

`TextEdit::layouter(fn) -> LayoutJob` 可完全接管文本排版：

- `LayoutJob.append(text, leading_space, TextFormat)`，每个片段可独立 `color` + `background` + `font_id` + `line_height`。
- 本项目用途：**JSON 语法着色**（按 token 给前景色）+ **搜索命中高亮**（给背景色），两者正交可叠加。
- 自定义 layouter 必须与默认行为一致：`wrap.max_width = wrap`、逐段 `line_height`、`keep_trailing_whitespace = true`，否则长文本不折行、看起来"消失"。

### 4.3 `Monospace` 等宽字体

等宽是**字体家族**（`FontFamily::Monospace`）的特性，不是字号。`TextStyle::Monospace` 只是碰巧映射到 family=Monospace 的 FontId。需要等宽用 `.monospace()` 或 `FontId::monospace(size)`。

---

## 5. 颜色 API（`ui.color_edit_button_*`）

8 个函数全是颜色选择器控件的便捷构造器：画一个显示当前色的方块按钮，点开弹取色面板，写回传入的 `&mut X`。差异在**颜色的数学表示**。

| 函数 | 类型 | 空间 | Alpha | 选型建议 |
|---|---|---|---|---|
| `srgba` | `&mut Color32` | sRGB | BlendOrAdditive | **最常用** |
| `hsva` | `&mut Hsva` | HSV | BlendOrAdditive | 色轮更直观 |
| `srgb` | `&mut [u8;3]` | sRGB | 无 | 不透明 |
| `rgb` | `&mut [f32;3]` | **线性 RGB** | 无 | 渲染数学用 |
| `srgba_premultiplied` | `&mut [u8;4]` | sRGB | 预乘 | GPU 合成 |
| `srgba_unmultiplied` | `&mut [u8;4]` | sRGB | 非预乘 | "不确定 premultiplied 就用这个" |
| `rgba_premultiplied` | `&mut [f32;4]` | **线性** | 预乘 | 线性浮点 |
| `rgba_unmultiplied` | `&mut [f32;4]` | **线性** | 非预乘 | "不确定就用这个" |

三个维度：
1. **空间**：带 `s` = sRGB；不带 `s` = 线性 RGB（`rgb`/`rgba`）；`hsva` = HSV。
2. **Alpha**：无 alpha / `BlendOrAdditive`（premultiplied 版）/ `OnlyBlend`（unmultiplied 版）。
3. **预乘**：premultiplied = RGB 已乘 alpha（GPU 用）；unmultiplied = RGBA 独立（设计/直觉用）。

> 易错：`rgb`/`rgba` 是**线性**空间，把 sRGB 数值误传进去会偏色。

---

## 6. 即时模式与动画

- **没有持久控件树**：每帧重建 UI。控件存在 = 这一帧调用了它的构建代码。
- **动画 = 每帧按真实时间插值 + 整帧重画**：用 `ctx.input(|i| i.time)` 算进度，过缓动函数；动画未结束调 `ctx.request_repaint()`。
- **帧率**：空闲时 0 FPS（事件驱动睡眠）；动画/交互中连续重绘，受 vsync 限制 ≈ 显示器刷新率，或受单帧耗时限制更低。
- **会顿的根因**：单帧工作量太大导致掉帧，或自定义动画忘记持续请求重绘——前者靠后台线程，后者记得 `request_repaint`。
- **基于时间而非帧数**的插值保证掉帧时动画时间线依然正确。

---

## 7. 方法解析与 `Deref`

### 7.1 同名方法优先级

- 类型**自身的固有方法 > deref 链上的方法**。`Ui` 有同名方法时不会再解引用到 `Context`，就近命中。
- 想调用被遮蔽的版本：`Context::foo(&ui)` / `ui.ctx().foo()` / `(*ui).foo()`。

### 7.2 一个类型只能有一个 `Deref`

coherence 规则禁止同类型实现两个 `Deref`（`Target` 必须唯一）。deref **链**（A→B→C）允许，同名方法取近的，永不二义。

### 7.3 trait 方法同名才会报歧义

固有方法或 deref 链靠"从近到远命中即停"决定；真正会报 `multiple applicable items` 的是**两个在作用域内的 trait 提供同名方法**，需用完全限定语法 `TraitX::foo(&x)`。

---

## 8. 常用容器与布局

- **面板顺序**：`Panel::top/bottom/left/right` 先于 `CentralPanel` 声明；面板按**每帧首次 show 的顺序**从可用区域切割。
- `ui.horizontal()` 超出不换行不滚动（直接裁切）；换行用 `horizontal_wrapped`。
- **同一帧内 `Grid::new(id)` 的 id 必须唯一**，复用固定 id 会报红。
- `Response::on_hover_text` 会消费并返回 Response，链式调用后接返回值再复用。
- `ComboBox::.width()` 只约束收起按钮宽度，弹层自适应；`.height()` 限弹层最大高。
- `Column::clip(true)` 会把 cell 的 wrap_mode 改成 Truncate。

---

## 9. 输入与事件

- **Ctrl+C/X/V 不产生按键事件**：egui-winit 在 winit 层转成 `Event::Copy/Cut/Paste` 后 return，`Key::C` 的 `key_pressed` 是死代码。自定义复制必须监听 `Event::Copy/Cut`。
- **Esc 帧首被没收焦点**：0.36 `Focus::begin_pass` 发现无修饰 Esc 立即 `focused_widget = None`，帧内 `focused()==Some(id)` 恒 false。正确判据：`resp.lost_focus() && input.key_pressed(Escape)`。
- 滚轮事件读 `ctx.input(|i| i.raw.events)` 的离散 `Event::MouseWheel { unit, delta }`，帧间状态用 `ctx.data_mut(|d| d.insert_temp(id, v))`。
- `scroll_delta` 语义是反的（"inverted"，应用时 `offset -= delta`，正值 = 向上看）。

---

## 10. 主题（0.36）

**切换深浅主题必须用 `ctx.set_theme(Theme::Dark/Light)`，不能用 `ctx.set_visuals()`**：0.36 是 ThemePreference（默认 System，跟随 `raw.system_theme`）+ dark_style/light_style 双 Style 模型；`set_visuals` 只替换"当前 theme 对应那份 style"的 visuals、不切换 preference，在跟随系统的浅色机器上首帧会被拉回浅色。同步原生标题栏用 `ctx.send_viewport_cmd(ViewportCommand::SetTheme(SystemTheme::Dark/Light))`。

---

## 11. 自绘可交互组件（hex 视图）要点

字符位置 ↔ 字节 精确映射时 TextEdit 做不到，改用：

- `ui.allocate_space(wanted)` → 解构 `(Id, Rect)`；
- `ui.interact(rect, 固定 id, Sense::click_and_drag())` 显式绑 id；
- `request_focus`、焦点判断、拖拽状态全部用**同一个稳定 id**（id 不一致时 Ctrl+C/焦点判断静默失效）；
- 临时数据类型必须 `Clone`；
- 只绘制与 `painter.clip_rect()` 相交的行；文本用缓存 `Galley` + `painter.galley`，指纹含内容/配置/`dark_mode`/`pixels_per_point`。

---

## 12. 字体与 CJK

- `FontData::from_owned(vec)` 插入 fonts 后，把字体名 push 进 Proportional/Monospace 两个族的 fallback 链。
- 运行时按平台动态查找，不写死路径。雅黑/Noto CJK 同时含西里尔字形，俄语 UI 无需额外字体。
- egui 内置字体（Ubuntu/NotoEmoji/emoji-icon/Hack）随二进制分发，优先级高于平台 CJK 字体。

---

## 13. 踩坑速查表

| 问题 | 原因 | 解决 |
|---|---|---|
| `egui_extras` 开 `table` feature 报错 | 0.36.2 表格已内置 | 删除 Cargo.toml 中该 feature |
| hover 表格 Value 弹大块空白 tooltip | `clip(true)` 改 wrap 为 Truncate，Label 默认 `show_tooltip_when_elided=true` 把截断全文塞进 tooltip；选中行白字+白底 | 自写 `elide_tooltip`：`fonts.layout_no_wrap` 实测宽度，**确被截断才** `on_hover_text`，截 512 字 + "…"，颜色显式 `visuals().text_color()` |
| hex 选区拖到视口边缘自动滚动方向反了 | `scroll_delta` 语义"inverted" | 压上边缘传正、下边缘传负 |
| JSON 导出使左栏动画式撑到最大宽 | 固定估值预留控件宽度导致恒定溢出 8px，SidePanel 每帧把期望尺寸写回 PanelState | `horizontal` + 内层 `right_to_left`：控件先从右占位，TextEdit 最后取 `available_width()` 填满 |
| 详情折叠 hex 时文本未占满空间 | `card_content_heights` 文本自然高度被永久 `.min(16*line_h)`，折叠 hex 后仍锁 16 行 | 16 行封顶只在无限预算估算时生效，实际渲染用真实高度参与分配 |
| JSON 详情文本区高度不对 | Auto 下 `guess` 返回紧凑原文（1 行），渲染却用 pretty（多行），估算与渲染文本不一致 | 提前算 `display_text`（Auto 命中结构化 JSON 时为 pretty），估算与渲染共用 |

---

## 14. 一帧的完整旅程（eframe + glow 后端）

以本项目 `eframe::run_native`（glow 渲染）为参考，一帧从生到死共 9 站（行号基于 0.36.2）：

| 站 | 位置 | 发生什么 | 关键源码 |
|---|---|---|---|
| 0 | 启动 | `run_native` 创建 winit 窗口 + OpenGL 上下文（glutin）+ `egui::Context`，三者存活整个程序周期 | `main.rs` → eframe `native/run.rs` |
| 1 | egui-winit | winit 的 `WindowEvent` 被翻译成 egui `Event`（MouseInput/CursorMoved/KeyboardInput/Ime/拖入文件），**攒在 State 里不触发绘制** | `egui-winit/src/lib.rs` `on_window_event`（L295） |
| 2 | eframe | winit `RedrawRequested` → `run_ui_and_paint`；`take_egui_input` 把攒的事件一次性打包成 `RawInput`（取走即清空），附 time/screen_rect/pixels_per_point | `glow_integration.rs` L566、L620 |
| 3 | `Context::run_ui` | 帧心脏：`loop { begin_pass → run_ui(self) → end_pass }`，`request_discard` 才多跑一轮（多 pass 布局），受 `max_passes` 限制；根 Ui = `LayerId::background()` + viewport_rect | `context.rs` L794/824-890，根 Ui L802-808；App 的 `update()` 挂在 `run_ui` |
| 4 | `begin_pass` | 用 RawInput 造 `InputState`：事件合成为语义（点击/双击/速度）；this_pass/prev_pass 交换、过期 Id 清理、动画时钟推进 | `context.rs` L962 → L418；`update()` 里 `ctx.input()` 是完整一帧快照 |
| 5 | `update()` | widget → `allocate_space`（Placer 记 WidgetRect）→ painter 把 `Shape` **存进 Context 的 graphics 图层列表**；本站没有任何像素被画 | `ui.rs`/`painter.rs` |
| 6 | `end_pass` | ①字体图集 glyph 增量 → `textures_delta`；②`graphics.drain()` 按图层顺序倒出全部 `ClippedShape`；③对比前后两帧 widget 矩形，变了自动 `request_repaint`；④打包 platform_output（剪贴板/光标/IME/accesskit） | `context.rs` L2443 → 私有 L2664（字体 L2675-2681、drain L2719、widget 对比 L2725-2750） |
| 7 | tessellate | **CPU 翻译官**：Shape → 三角形网格 `Mesh`（顶点 pos+uv+color）。圆→三角扇、路径→三角条、文字→glyph 四边形（uv 指字体图集）、抗锯齿→边缘羽化顶点。一帧 CPU 开销大头，优化核心是少生成 Shape | eframe `glow_integration.rs` L783 → epaint `tessellator.rs` `tessellate_shapes`（L2220） |
| 8 | egui_glow | 先 `set_texture` 上传/更新字体图集；再逐 Mesh 上传 VBO/EBO，`gl::draw_elements` 画三角形——整个 UI 本质是"画贴图三角形" | `egui_glow/src/painter.rs` L356/400/494/510 |
| 9 | 上屏+睡眠 | glutin `swap_buffers` 上屏 → eframe 处理 repaint 请求 → 空闲时 winit 线程挂起等 OS 消息（空闲帧率≈0 的实现处） | `glow_integration.rs` 收尾 + winit 事件循环 |

新旧知识挂钩：动画时钟=第 4 站推进+第 9 站唤醒；Esc 焦点 bug=第 4 站 `Focus::begin_pass` 先扫原始输入；`Ctrl+C` 走 `Event::Copy`=第 1 站翻译产物；Panel 宽度回写/`remember_min_rect`=第 5/6 站之间；性能优化抓手=第 7 站少生成 Shape。

---

## 15. `context.rs` 功能总表

骨架：`pub struct Context(Arc<RwLock<ContextImpl>>)`（L723）。所有 162 个公开方法都是 `&self`，修改走内部 RwLock 的 `read/write` 闭包门面——这是 `Ui` 只实现 `Deref` 不实现 `DerefMut` 的根源。

`ContextImpl` 字段两层：全局共享（fonts/memory/animation_manager/tex_manager/plugins/loaders/os，L375-415）+ 每视口独立（`viewports: ViewportIdMap<ViewportState>`，内含 input/output/graphics/this_pass/prev_pass/repaint）。单窗口应用恒走 ROOT 视口。

重绘状态机（`impl ContextImpl` "Repaint-logic"，L97-185，每视口 `ViewportRepaintInfo`）三个设计：

- `request_repaint_after` 零延迟请求会排**两次**重绘（`outstanding=1`），兜底 frame-delayed response（L138-141）
- delay 预扣 `predicted_dt` 一帧耗时防过冲（L149-152）
- 新 delay 只有比当前计划早才调用 `request_repaint_callback`（eframe 启动时经 `set_request_repaint_callback` L1961 挂入"唤醒 winit"函数）；`begin_pass` 时若仍有积压立即以 ZERO 延迟再通知（L110-123）

| # | 功能块（行号） | 核心方法 | 要点 |
|---|---|---|---|
| 1 | 帧生命周期 794/913/962/2443/2859 | `run_ui` `run_logic` `begin_pass` `end_pass` `tessellate` | 见第 14 章；`run_logic`=窗口不可见时只跑逻辑不建 Ui |
| 2 | 多 pass 1992/2011 | `request_discard` `will_discard` | 本帧作废重来，上限 `max_passes`，`run_dyn` 的 loop 为此存在 |
| 3 | 状态门面 991-1147 | `input` `memory(_mut)` `data(_mut)` `graphics(_mut)` `output(_mut)` `fonts(_mut)` `options(_mut)` | read/write 闭包包装；`data_mut`=PanelState 持久化落点 |
| 4 | 重绘 1821-1961 | `request_repaint(_after/_secs)` `repaint_causes` | 状态机公开入口，动画每帧自动调 |
| 5 | Id/交互 1163/1355/1389/1572 | `check_for_id_clash` `read_response` `register_widget_info` | Id 撞车=警告+红框；`read_response(id)` 跨控件取当帧 Response |
| 6 | 拖拽 4192-4262 | `dragged_id` `is_being_dragged` `stop_dragging` | hex 拖选底层机制 |
| 7 | 图层 1587/3065-3138 | `layer_painter` `move_to_top` `layer_id_at` `set_transform_layer` | `layer_id_at`=点空白关弹窗；`move_to_top`=窗口置顶 |
| 8 | 指针/焦点 2943-3012 | `egui_wants_*` `text_edit_focused` `any_popup_open` | Ctrl+C 焦点判定源 |
| 9 | 时间 1616/1751-1804 | `time` `cumulative_frame_nr` `cumulative_pass_nr` | 动画时间源；frame 含 discard 重跑 |
| 10 | 动画 3191-3282 | `animate_bool(_with_time)` `animate_value_with_time` | 走独立 `AnimationManager`，不占 memory |
| 11 | 字体 2106/2129 | `set_fonts` `add_font` | 项目 CJK 加载走 `set_fonts`；`add_font` 运行时热插 |
| 12 | 主题样式 2152-2280 | `set_theme` `global_style(_mut)` `style_mut_of(theme)` `set_visuals_of` | 每主题一份 Style；set_visuals 踩坑的 API 设计根源 |
| 13 | 缩放 2288-2337 | `pixels_per_point` `set_pixels_per_point` `zoom_factor` | 独立缩放系统 |
| 14 | 纹理 2390/2417 | `load_texture` `tex_manager` | 返回 `TextureHandle` |
| 15 | 资源加载 3719-3934 | `try_load_image` `forget_image` `has_pending_images` | 异步图片管线，未传完自动续帧 |
| 16 | 视口 3949-4116 | `send_viewport_cmd` `show_viewport_deferred/immediate` | 多窗口；项目 SetTheme/Close 走它 |
| 17 | 输出/OS 1627-1719 | `set_cursor_icon` `open_url` `copy_text` | copy_text=手动写剪贴板 |
| 18 | 插件 2026-2087 | `add_plugin` `on_begin_pass` `on_end_pass` | pass 前后钩子 |
| 19 | 无障碍 3684-3706 | `enable_accesskit` | end_pass 导出无障碍树（L2694-2717） |
| 20 | 调试 UI 3289-3664 | `settings_ui` `memory_ui` `texture_ui` `inspection_ui` | 一行挂出调试面板 |

一句话：context.rs = 一把全局读写锁 + 两层状态（全局/每视口）+ 重绘状态机 + 帧引擎 + 门面 API；Ui/Placer/Widget 每帧打工，跨帧状态全汇到这把锁里。

"Borrows parts of Context" 节（L971+）几十个小入口是被逼的：状态在 RwLock 后面引用出不了方法（guard return 即 drop，`&InputState` 悬垂），只剩两条路——闭包（`input(|i| ...)`，锁内借引用零拷贝；input 意外是 write 锁，多视口可能现场 default 创建）或拷出来（`style()`→Arc 克隆、`time()`→Copy）。按字段拆是为了各入口精确声明锁级别与返回形态。⚠️ 唯一深坑：**闭包体内禁止再碰 ctx 任何加锁方法（含另一个 Arc 引用），RwLock 不可重入 = 死锁**。`#[inline]`+单态化 → 运行时零成本；碎是"可克隆线程安全句柄"的 API 代价。

`run_ui`（L794）是"跑一帧 UI"的公开入口（eframe 在 glow_integration L1674 调它），产出 FullOutput（Shape 描述），真正的画在其后 tessellate+GPU。它拿到泛型闭包后立即委托 `run_ui_dyn(&mut dyn FnMut)`——重型 `run_dyn` 帧循环只编译一份，属"thin generic wrapper + dyn core"代码体积优化：高频小操作（read/write）用泛型内联，低频大块头（帧循环）用 dyn 控体积。兄弟入口：`run_logic`（窗口不可见只跑逻辑）、手动 begin_pass/end_pass。

**空闲睡眠链**（"没事就不画不占资源"的实现）：UI `request_repaint_after` → egui 重绘状态机记账 → end_pass 打包 repaint_delay → eframe 预注册的回调（glow_integration.rs L303-315：`proxy.send_event(RequestRepaint{when: now+delay})`，依赖倒置——egui 不知 winit 存在）→ winit `wait_until(when)` Wait 模式 → OS 内核级睡眠（Windows=GetMessage，不被调度，0% CPU / 0 GPU present）→ 到点 RedrawRequested 跑一帧 → UI 再续约则循环，无人续约则永久睡。动画=每帧自续约；静态 UI=睡到鼠标动。对照：游戏/rAF 持续轮询空转，egui 是脉冲式唤醒。

插件系统（plugin.rs，0.36 新）：`Plugin` trait 六钩子——`setup`（注册时一次）、`on_begin_pass`/`on_end_pass`（每 pass，可画 UI）、`input_hook`（篡改 RawInput）、`output_hook`（篡改 FullOutput）、`on_widget_under_pointer`（debug 构建）。注册表 `HashMap<TypeId, Arc<Mutex<_>>>`，每类型一个插件（TypeMap 思路），`ctx.plugin::<T>()` 拿带锁句柄，钩子夹在 run_ui_dyn 的根 Ui 两侧。内置仅 4 个：CallbackPlugin（托管 `ctx.on_begin_pass` 回调，accesskit 走这里）、DebugTextPlugin（debug_text 悬浮调试文字）、LabelSelectionState（Label 文本拖选）、DragAndDrop（拖放载荷）。accesskit 与 image loader 不算插件（前者走回调、后者另有 loaders）。应用可 `ctx.add_plugin` 自写插件做每帧自动 UI/输入输出篡改。

---

## 16. `painter.rs`：画笔——只记账，不渲染（569 行，最小的一个）

Painter 是 Ui 的"手"：把 `(clip_rect, Shape)` append 进当帧 PaintList，像素化在 end_pass→tessellate→GPU。doc L19 定性"A Painter never outlives a single frame/pass"——画笔与 ShapeIdx 都只活一帧，shape 是瞬时数据别存。5 字段：ctx、pixels_per_point（**缓存 PPE 免走锁**）、layer_id（z 序）、clip_rect（裁剪）、fade_to_color+opacity_factor（画笔级调色，**禁用态灰化 UI 就是它实现的**）。Clone 廉价（Context 是 Arc）。

API 三层，真干活的只有 `add`：`transform_shape`（fade/opacity 逐色调整，跳过 PLACEHOLDER 占位色）→ `paint_list`（graphics_mut→图层列表 append）。要点：**with_clip_rect 做交集，裁剪只能缩不能放**（set_clip_rect 放大有警告）；extend 批量、set 按 ShapeIdx 帧内改、for_each_shape 读回。原语层（line/circle/rect×3/arrow/image）全是 `Shape::xxx`+add 语法糖；文本两步走 `layout`（→Arc<Galley> 可缓存）→`galley`。调试三件套：debug_rect/debug_text/error（🔥）。应用场景：`ui.painter()`/`painter_at()` 自定义绘制（网格线/图表），坐标是屏幕坐标非 Ui 内坐标。

**Tessellate（三角化）**：GPU 只会画三角形，`Vec<ClippedShape>`（意图）→ epaint::Tessellator（CPU）→ `Vec<ClippedMesh>`（顶点+索引）→ GPU。发生在 run_ui 返回后、交后端前，典型耗时 0.1-1ms（CPU 侧最重一笔）。圆→扇形带、描边→挤出四边形条、文字→每字形一个四边形（字体图集）。**抗锯齿=feathering**：每条边多挤一圈 alpha 渐变三角形模拟软边（非 MSAA），最贵的环节，`options.feathering=false` 提速换锯齿。逃生门：`Shape::Callback`（epaint paint_callback.rs）跳过 tessellation，GPU 阶段直接执行原生渲染闭包（wgpu/glow 画 3D/视频）。另外 egui **整窗全量重绘、无脏矩形**（立即模式刻意为之）：省力在帧调度（空闲零帧）而非帧内抠区域；集显画 UI 全帧是数量级富余，tessellate 才是 CPU 瓶颈。

---

## 附录 A. `ui.rs` 功能总表（按功能块）

| # | 功能块（行号） | 核心方法 | 功能要点 | 注意点 / 坑 |
|---|---|---|---|---|
| 1 | 创建与基础 L101–670 | `new`、`new_child`、`ctx`、`painter`、`style`/`style_mut`、`spacing`、`visuals`、`layout`、`layer_id`、`clip_rect` | 构造顶层/子 Ui；访问 Context、Painter、样式、布局、裁剪区；控制启用/可见/不透明度 | `new_child` 不自动占父空间，需手动报备或用容器封装；`style_mut` 触发 COW（读用 `style()`）；`Ui` 自动 `Deref` 到 `Context`，无 `DerefMut` |
| 2 | Sizes etc L675–880 | `min_rect`/`min_size`、`max_rect`、`set_max_*`、`set_min_*`、`take_available_*`、`available_*`、`available_rect_before_wrap` | 查询/约束 Ui 的最小、最大、可用尺寸 | `min_rect` 是已占区域，`max_rect` 是可用边界；`take_available_space` 会让父光标推进；`available_rect_before_wrap` 是"填满整行"的关键 |
| 3 | Id creation L882–903 | `make_persistent_id`、`next_auto_id`、`auto_id_with`、`skip_ahead_auto_ids` | 生成稳定 id（跨帧存状态）或自动 id（当帧交互） | 持久状态用 `make_persistent_id`；自动 id 依赖前面不增删控件，不适合存状态；`skip_ahead_auto_ids` 用于分支条件下保持 id 稳定 |
| 4 | Interaction L905–1116 | `interact`、`response`、`close`/`should_close`、`ui_contains_pointer` | 让某 rect 可交互；取 Ui 自身响应；标记/询问关闭 | `response()` 读的是**上一帧** widget rect（本帧为 `Rect::NOTHING`）；关闭信号由宿主"下一帧不创建"生效，控件本身不消失 |
| 5 | Allocating space L1118–1378 | `allocate_space`、`allocate_rect`、`allocate_exact_size`、`allocate_at_least`、`advance_cursor_after_rect`、`allocate_ui` | 向 Placer 申请矩形、推进光标、分配子 Ui 区域 | `allocate_space` 只占位不交互；`allocate_rect` 占指定矩形并交互；`advance_cursor_after_rect` 手动报备子区域占用 |
| 6 | Scrolling L1380–1503 | `scroll_to_rect`、`scroll_to_cursor`、`scroll_with_delta`（+ 各 `_animation` 版） | 滚动到指定矩形/光标/按增量滚动 | 滚动位置存 Context 的 `area_visible_rect`（key=layer_rect）；`scroll_with_delta` 用 `-delta`，方向与直觉相反；动画版需持续 `request_repaint` |
| 7 | Adding widgets L1505–2038 | `add`、`add_sized`、`place`/`put`、`add_enabled`/`add_visible`、`label`、`heading`、`monospace`、`button`、`checkbox`、`radio_value`、`selectable_value`、`text_edit_*`、`separator`、`spinner`、`image` | 添加各类控件；变体糖（粗体/等宽/标题色等） | `add` = `widget.ui(self)`，真正"添加"在控件内部；Label 只占内容大小；`add_sized` 继承父布局方向，`put`/`place` 固定 TopDown；`radio_value`/`selectable_value` 内部比对值并回写 |
| 8 | Colors L2040–2134 | 8 个 `color_edit_button_*` | 颜色选择器控件，按颜色表示形式区分 | 三维：空间（sRGB/线性/HSV）× alpha（无/BlendOrAdditive/OnlyBlend）× 预乘；`rgb`/`rgba` 是**线性**空间，误传 sRGB 数值会偏色；不确定 premultiplied 用 unmultiplied |
| 9 | Containers / Sub-uis L2135–2768 | `scope`、`group`、`indent`、`horizontal`/`vertical`、`with_layout`、`columns`、`dnd_*`、`with_visual_transform` | 创建子区域（独立样式/布局/裁剪）、分列、拖拽源/放置区、坐标变换 | `scope` 隔离样式改动；`horizontal` 超出裁切不换行（用 `horizontal_wrapped`）；`with_layout` 改布局方向只影响子区域；DnD 的 Payload 类型需跨拖拽源/放置区一致 |
| 10 | Menus L2769–2880 | `menu_button`、`menu_image_button`、`menu_image_text_button` | 弹出式菜单按钮 | 菜单项放闭包内；关闭由框架自动管理（`should_close`），不需手动调 |
| 11 | Debug L2881– | `debug_paint_cursor` | 调试用：绘制当前光标位置 | 仅开发调试，不要用于发布逻辑 |

### 几条贯穿全表的脉络

- **添加控件闭环**：`add(widget)` → `widget.ui(self)` → 内部 `allocate_space` 占位 + `interact` 注册交互 + `painter` 绘制。
- **填满整列**：Label 做不到；用 `allocate_rect(available_rect_before_wrap, sense)` + 手动画字，或 `put(rect, widget)`。
- **整行可点**：表格靠 `TableBuilder.sense(click())` 把 sense 下传到 cell 背景；表头要自己实现。
- **关闭容器**：`set_close()` 只打标记，真正消失是宿主下一帧不再创建该 Ui。
- **滚动**：改 Context 里 `area_visible_rect` 的 Rect，`scroll_with_delta` 反向平移。
