// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 右栏详情面板状态：Key/Value 两张卡片各自的状态 + 多值导航状态。
//! 本文件只放数据与纯状态迁移（分段翻页/偏移跳转），不涉及 egui 渲染。

use crate::fmt::{self, DecodeMode};
use crate::i18n::tr;
use crate::ui::MsgLevel;

/// 右侧多值导航：每页懒加载的值个数。
pub const DUP_PAGE_SIZE: usize = 100;

/// 卡片角色。两者差异集中在这里：Key 不做 Auto 猜型、可收藏；
/// Value 支持多值导航与"猜测格式另存"。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Key,
    Value,
}

impl Side {
    /// 是否 Key 卡片。
    pub fn is_key(self) -> bool {
        self == Side::Key
    }

    /// Auto 模式下是否允许特殊格式猜测。Key 短小，极易被凑巧的结构误判，
    /// 故禁止；Value 是数据载荷，允许结构化/压缩/proto 猜测。
    pub fn allows_guess(self) -> bool {
        self == Side::Value
    }
}

/// hex 视图选区：内容指纹（换段/换值/换行即作废）+ 字节端点（顺序无关）。
#[derive(Default)]
pub struct HexSel {
    /// `(段偏移, 段长度, 多值序号, 选中行)`；None 表示从未渲染
    pub fp: Option<(usize, usize, Option<usize>, usize)>,
    /// 字节全局序号端点
    pub sel: Option<(usize, usize)>,
}

/// 单张卡片（Key 或 Value）的状态。
pub struct CardState {
    /// 解码格式
    pub mode: DecodeMode,
    /// 当前段起始偏移
    pub seg_off: usize,
    /// 偏移跳转输入框
    pub seg_input: String,
    /// 大字段内容搜索输入（文本或 hex(...)）
    pub blob_input: String,
    /// 最近一次搜索命中区间（全局偏移 start, len），用于 hex 高亮
    pub blob_hit: Option<(usize, usize)>,
    /// 搜索命中序号与总数（1 起，VSCode 风格 n/m）
    pub blob_matches: Option<(usize, usize)>,
    /// 待滚动的命中字节位置（全局偏移，消费一次后清空）
    pub blob_scroll: Option<usize>,
    /// 折叠视图状态（显示文本指纹 + 已折叠行号；指纹不符自动重置）
    pub fold: Option<(u64, Vec<usize>)>,
    /// 折叠视图选区（显示文本指纹 + 字节端点）
    pub fold_sel: Option<(u64, Option<(usize, usize)>)>,
    /// hex 视图选区
    pub hex: HexSel,
}

impl Default for CardState {
    fn default() -> Self {
        Self {
            mode: DecodeMode::Auto,
            seg_off: 0,
            seg_input: String::new(),
            blob_input: String::new(),
            blob_hit: None,
            blob_matches: None,
            blob_scroll: None,
            fold: None,
            fold_sel: None,
            hex: HexSel::default(),
        }
    }
}

impl CardState {
    /// 换值/换段后失效的搜索、分段与 hex 选区；格式、搜索输入与折叠状态保留。
    pub fn reset_transient(&mut self) {
        self.seg_off = 0;
        self.seg_input.clear();
        self.blob_hit = None;
        self.blob_matches = None;
        self.blob_scroll = None;
        self.hex.sel = None;
        self.hex.fp = None;
    }

    /// 换行/换表/关库：格式回到 Auto，搜索与分段输入全部清空
    /// （折叠状态按内容指纹失效；hex 选区显式清除）。
    pub fn reset_all(&mut self) {
        self.mode = DecodeMode::Auto;
        self.blob_input.clear();
        self.reset_transient();
    }
}

/// 多值（DUP_SORT）导航状态。
pub struct DupNavState {
    /// 当前 Key 的值总数
    pub total: usize,
    /// 当前选中值的全局序号（0 起）
    pub index: usize,
    /// values 中第一个值的全局序号（跨页加载用）
    pub page_start: usize,
    /// 当前值页的数据（最多 DUP_PAGE_SIZE 个）
    pub values: Vec<Vec<u8>>,
    /// 序号跳转输入（1 起）
    pub jump_input: String,
    /// 值内容搜索输入（文本或 hex(...)）
    pub search_input: String,
}

impl Default for DupNavState {
    fn default() -> Self {
        Self {
            total: 1,
            index: 0,
            page_start: 0,
            values: Vec::new(),
            jump_input: String::new(),
            search_input: String::new(),
        }
    }
}

impl DupNavState {
    /// 回到"空表"状态（换行/换表/关库）。
    fn reset(&mut self) {
        self.total = 1;
        self.index = 0;
        self.page_start = 0;
        self.values.clear();
        self.jump_input.clear();
        self.search_input.clear();
    }
}

/// 右侧详情面板状态：两张卡片 + 多值导航 + 全局 hex 视图配置。
pub struct DetailState {
    /// Key 卡片状态
    pub key: CardState,
    /// Value 卡片状态
    pub val: CardState,
    /// 多值导航状态（仅 Value 卡片）
    pub dup: DupNavState,
    /// hex dump 是否显示地址列
    pub show_addr: bool,
    /// hex dump 是否显示十六进制列
    pub show_hex: bool,
    /// hex dump 是否显示 ASCII 列
    pub show_ascii: bool,
    /// 详情文本行号开关
    pub show_text_line_numbers: bool,
    /// HexView 显示行号开关
    pub show_hex_line_numbers: bool,
    /// hex dump 紧凑格式：HEX 列字节间不加空格
    pub hex_compact: bool,
    /// hex dump 行宽（每行字节数：4/8/16/32）
    pub hex_width: usize,
    /// Protobuf 载入 .proto 失败的错误信息（会话态，成功载入后清空）
    pub proto_err: Option<String>,
}

impl Default for DetailState {
    fn default() -> Self {
        Self {
            key: CardState::default(),
            val: CardState::default(),
            dup: DupNavState::default(),
            show_addr: true,
            show_hex: true,
            show_ascii: true,
            show_text_line_numbers: false,
            show_hex_line_numbers: false,
            hex_compact: false,
            hex_width: fmt::DEFAULT_HEX_WIDTH,
            proto_err: None,
        }
    }
}

impl DetailState {
    /// 清空多值与分段状态（换行/换表/关库时调用）。
    /// 详情卡片排版是"当前行"的临时查看选择，换行即回到自动；
    /// 同一 Key 内翻多值走 dup_goto 不调本函数，格式得以保持。
    pub fn clear(&mut self) {
        self.key.reset_all();
        self.val.reset_all();
        self.dup.reset();
    }

    /// 卡片当前的解码格式。
    pub fn mode_of(&self, side: Side) -> DecodeMode {
        match side {
            Side::Key => self.key.mode,
            Side::Value => self.val.mode,
        }
    }

    /// 写回卡片的解码格式。
    pub fn set_mode(&mut self, side: Side, mode: DecodeMode) {
        match side {
            Side::Key => self.key.mode = mode,
            Side::Value => self.val.mode = mode,
        }
    }

    /// 卡片当前段起始偏移。
    pub fn seg_off(&self, side: Side) -> usize {
        match side {
            Side::Key => self.key.seg_off,
            Side::Value => self.val.seg_off,
        }
    }

    /// 取出待滚动命中位置（消费一次后清空），避免同一目标每帧重复抢滚动。
    pub fn blob_scroll_take(&mut self, side: Side) -> Option<usize> {
        match side {
            Side::Key => self.key.blob_scroll.take(),
            Side::Value => self.val.blob_scroll.take(),
        }
    }

    /// 上/下翻 `pages` 个段（每段 fmt::PAGE_BYTES 字节），自动夹到有效范围。
    pub fn seg_step(&mut self, side: Side, total: usize, pages: isize) {
        let cur = (match side {
            Side::Key => self.key.seg_off,
            Side::Value => self.val.seg_off,
        } / fmt::PAGE_BYTES) as isize;
        let max_seg = total.saturating_sub(1) / fmt::PAGE_BYTES;
        let target = (cur + pages).clamp(0, max_seg as isize) as usize;
        let off = target * fmt::PAGE_BYTES;
        match side {
            Side::Key => self.key.seg_off = off,
            Side::Value => self.val.seg_off = off,
        }
    }

    /// 跳至指定偏移：十进制或 0x 十六进制；向下对齐到段边界并夹到末尾段。
    /// 返回（级别, 状态栏消息）。
    pub fn seg_jump(&mut self, side: Side, total: usize) -> (MsgLevel, String) {
        let t = tr();
        let input = match side {
            Side::Key => self.key.seg_input.trim(),
            Side::Value => self.val.seg_input.trim(),
        };
        let parsed = if let Some(h) = input
            .strip_prefix("0x")
            .or_else(|| input.strip_prefix("0X"))
        {
            usize::from_str_radix(h, 16)
        } else {
            input.parse::<usize>()
        };

        match parsed {
            Ok(v) if v < total => {
                let off = (v / fmt::PAGE_BYTES) * fmt::PAGE_BYTES;
                match side {
                    Side::Key => self.key.seg_off = off,
                    Side::Value => self.val.seg_off = off,
                }
                (MsgLevel::Info, t.seg_ok(off))
            }
            Ok(v) => (MsgLevel::Warn, t.seg_range_msg(v, total)),
            Err(_) => (MsgLevel::Warn, t.seg_bad.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CardState, DUP_PAGE_SIZE, DetailState, Side};
    use crate::fmt;

    #[test]
    fn line_number_options_default_to_off() {
        let state = DetailState::default();
        assert!(!state.show_text_line_numbers);
        assert!(!state.show_hex_line_numbers);
    }

    #[test]
    fn clear_resets_modes_and_inputs() {
        let mut st = DetailState::default();
        st.key.mode = fmt::DecodeMode::Json;
        st.key.seg_off = fmt::PAGE_BYTES;
        st.key.seg_input = "0x10".into();
        st.key.blob_input = "abc".into();
        st.key.blob_hit = Some((1, 3));
        st.key.fold = Some((7, vec![1]));
        st.key.hex.sel = Some((1, 3));
        st.key.hex.fp = Some((0, 4, Some(0), 0));
        st.dup.total = 9;
        st.dup.values = vec![b"v".to_vec()];

        st.clear();

        assert_eq!(st.key.mode, fmt::DecodeMode::Auto);
        assert_eq!(st.key.seg_off, 0);
        assert!(st.key.seg_input.is_empty());
        assert!(st.key.blob_input.is_empty());
        assert_eq!(st.key.blob_hit, None);
        // 折叠状态按内容指纹失效，不在 clear 里强行清空
        assert_eq!(st.key.fold, Some((7, vec![1])));
        assert_eq!(st.key.hex.sel, None);
        assert_eq!(st.key.hex.fp, None);
        assert_eq!(st.dup.total, 1);
        assert!(st.dup.values.is_empty());
    }

    #[test]
    fn seg_step_clamps_to_valid_range() {
        let mut st = DetailState::default();
        let total = fmt::PAGE_BYTES * 2 + 10;

        st.seg_step(Side::Value, total, -1);
        assert_eq!(st.val.seg_off, 0);

        st.seg_step(Side::Value, total, 1);
        assert_eq!(st.val.seg_off, fmt::PAGE_BYTES);

        st.seg_step(Side::Value, total, 5);
        assert_eq!(st.val.seg_off, fmt::PAGE_BYTES * 2);

        st.seg_step(Side::Key, total, 1);
        assert_eq!(st.key.seg_off, fmt::PAGE_BYTES);
    }

    #[test]
    fn seg_jump_parses_decimal_and_hex() {
        let mut st = DetailState::default();
        let total = fmt::PAGE_BYTES * 2;

        st.key.seg_input = "0x10".into();
        let (lvl, _) = st.seg_jump(Side::Key, total);
        assert!(matches!(lvl, crate::ui::MsgLevel::Info));
        assert_eq!(st.key.seg_off, 0);

        st.key.seg_input = format!("{}", fmt::PAGE_BYTES + 5);
        st.seg_jump(Side::Key, total);
        assert_eq!(st.key.seg_off, fmt::PAGE_BYTES);

        st.key.seg_input = "0x".into();
        let (lvl, _) = st.seg_jump(Side::Key, total);
        assert!(matches!(lvl, crate::ui::MsgLevel::Warn));
        assert_eq!(st.key.seg_off, fmt::PAGE_BYTES);
    }

    #[test]
    fn seg_jump_rejects_out_of_range() {
        let mut st = DetailState::default();
        let total = 10;
        st.val.seg_input = "10".into();
        let (lvl, msg) = st.seg_jump(Side::Value, total);
        assert!(matches!(lvl, crate::ui::MsgLevel::Warn));
        assert!(!msg.is_empty());
        assert_eq!(st.val.seg_off, 0);
    }

    #[test]
    fn reset_transient_keeps_format_and_input() {
        let mut card = CardState {
            mode: fmt::DecodeMode::Hex,
            blob_input: "needle".into(),
            seg_off: fmt::PAGE_BYTES,
            blob_matches: Some((2, 3)),
            ..CardState::default()
        };
        card.hex.sel = Some((2, 5));
        card.hex.fp = Some((0, 8, Some(0), 0));

        card.reset_transient();

        assert_eq!(card.mode, fmt::DecodeMode::Hex);
        assert_eq!(card.blob_input, "needle");
        assert_eq!(card.seg_off, 0);
        assert_eq!(card.blob_matches, None);
        assert_eq!(card.hex.sel, None);
        assert_eq!(card.hex.fp, None);
    }

    #[test]
    fn dup_page_size_is_stable() {
        // 跨页加载逻辑依赖该常量；改值需同步检查 dup_goto / dup_page_step
        assert_eq!(DUP_PAGE_SIZE, 100);
    }
}
