// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 卡片大字段的字节内容搜索（纯逻辑，不涉及 egui）。

use super::state::{DetailState, Side};
use crate::fmt;
use crate::i18n::tr;
use crate::ui::{MsgLevel, parse_bytes_input};

/// 从当前命中出发查找下一个命中（含回绕）。
/// 返回 `(位置, 是否回绕)`；无命中返回 None。
fn find_hit(
    bytes: &[u8],
    needle: &[u8],
    cur: Option<(usize, usize)>,
    forward: bool,
) -> Option<(usize, bool)> {
    if needle.is_empty() || needle.len() > bytes.len() {
        return None;
    }

    if forward {
        // 从当前命中末尾之后继续；无命中则从头搜
        let start = cur.map(|(p, l)| p + l).unwrap_or(0).min(bytes.len());
        match bytes[start..]
            .windows(needle.len())
            .position(|w| w == needle)
        {
            Some(i) => Some((start + i, false)),
            None if start > 0 => bytes[..start]
                .windows(needle.len())
                .position(|w| w == needle)
                .map(|p| (p, true)),
            None => None,
        }
    } else {
        // 从当前命中起点之前找；无命中则从末尾搜
        let start = cur.map(|(p, _)| p).unwrap_or(bytes.len()).min(bytes.len());
        match bytes[..start]
            .windows(needle.len())
            .rposition(|w| w == needle)
        {
            Some(p) => Some((p, false)),
            None if start < bytes.len() => bytes[start..]
                .windows(needle.len())
                .rposition(|w| w == needle)
                .map(|i| (start + i, true)),
            None => None,
        }
    }
}

/// 统计全部命中（不重叠），返回（`pos` 的序号 1 起, 总数）。
fn count_hits(bytes: &[u8], needle: &[u8], pos: usize) -> (usize, usize) {
    let mut total = 0usize;
    let mut cur_no = 0usize;
    let mut scan = 0usize;
    while scan + needle.len() <= bytes.len() {
        match bytes[scan..]
            .windows(needle.len())
            .position(|w| w == needle)
        {
            Some(rel) => {
                let p = scan + rel;
                total += 1;
                if p == pos {
                    cur_no = total;
                }
                scan = p + needle.len();
            }
            None => break,
        }
    }
    (cur_no.max(1), total)
}

impl DetailState {
    /// 在卡片完整字节中做字节子串搜索：文本按 UTF-8，其它格式查询先解析为字节。
    /// `forward=false` 向小偏移方向；以当前命中为起点跳过自身，主方向无命中回绕。
    /// 命中后对齐到所在段并记录高亮区间。返回（级别, 状态栏消息）。
    pub fn blob_search(&mut self, side: Side, bytes: &[u8], forward: bool) -> (MsgLevel, String) {
        let t = tr();
        let input = match side {
            Side::Key => self.key.blob_input.trim(),
            Side::Value => self.val.blob_input.trim(),
        }
        .to_string();
        if input.is_empty() {
            match side {
                Side::Key => self.key.blob_matches = None,
                Side::Value => self.val.blob_matches = None,
            }
            return (MsgLevel::Warn, t.blob_prompt.to_string());
        }

        let needle = match parse_bytes_input(&input) {
            Ok(b) => b,
            Err(e) => return (MsgLevel::Error, t.dup_bad_query(&e)),
        };

        if needle.is_empty() || needle.len() > bytes.len() {
            match side {
                Side::Key => self.key.blob_matches = None,
                Side::Value => self.val.blob_matches = None,
            }
            return (MsgLevel::Warn, t.blob_nomatch.to_string());
        }

        let cur = match side {
            Side::Key => self.key.blob_hit,
            Side::Value => self.val.blob_hit,
        };
        let Some((pos, wrapped)) = find_hit(bytes, &needle, cur, forward) else {
            match side {
                Side::Key => self.key.blob_matches = None,
                Side::Value => self.val.blob_matches = None,
            }
            return (MsgLevel::Warn, t.blob_nomatch.to_string());
        };

        // 命中后记录高亮区间、n/m 计数，并跳到命中所在段；
        // 置滚动目标，内容区滚动组件本帧消费后把命中行滚到可见
        let (cur_no, total) = count_hits(bytes, &needle, pos);
        let seg_off = (pos / fmt::PAGE_BYTES) * fmt::PAGE_BYTES;
        let card = match side {
            Side::Key => &mut self.key,
            Side::Value => &mut self.val,
        };
        card.blob_hit = Some((pos, needle.len()));
        card.blob_matches = Some((cur_no, total));
        card.seg_off = seg_off;
        card.blob_scroll = Some(pos);

        if wrapped {
            (MsgLevel::Info, t.blob_wrap(pos))
        } else {
            (MsgLevel::Info, t.blob_located(pos))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{count_hits, find_hit};
    use crate::ui::detail::state::{DetailState, Side};

    /// 构造带搜索输入的状态，跑一次搜索，返回 (命中区间, 段偏移)。
    fn run_search(input: &str, data: &[u8], fwd: bool) -> (Option<(usize, usize)>, usize) {
        let mut st = DetailState::default();
        st.val.blob_input = input.to_string();
        let _ = st.blob_search(Side::Value, data, fwd);
        (st.val.blob_hit, st.val.seg_off)
    }

    #[test]
    fn find_hit_forward_skips_self_and_wraps() {
        let bytes = b"xxabcxxabcxx";
        assert_eq!(find_hit(bytes, b"abc", None, true), Some((2, false)));
        assert_eq!(
            find_hit(bytes, b"abc", Some((2, 3)), true),
            Some((7, false))
        );
        assert_eq!(find_hit(bytes, b"abc", Some((7, 3)), true), Some((2, true)));
    }

    #[test]
    fn find_hit_backward_from_end() {
        let bytes = b"xxabcxxabcxx";
        assert_eq!(find_hit(bytes, b"abc", None, false), Some((7, false)));
        assert_eq!(
            find_hit(bytes, b"abc", Some((7, 3)), false),
            Some((2, false))
        );
        assert_eq!(
            find_hit(bytes, b"abc", Some((2, 3)), false),
            Some((7, true))
        );
    }

    #[test]
    fn find_hit_rejects_empty_or_oversized_needle() {
        assert_eq!(find_hit(b"abc", b"", None, true), None);
        assert_eq!(find_hit(b"abc", b"abcdef", None, true), None);
    }

    #[test]
    fn count_hits_is_non_overlapping() {
        assert_eq!(count_hits(b"aaaa", b"aa", 0), (1, 2));
        assert_eq!(count_hits(b"aaaa", b"aa", 2), (2, 2));
        assert_eq!(count_hits(b"ab--ab--ab", b"ab", 4), (2, 3));
        assert_eq!(count_hits(b"ab--ab--ab", b"ab", 8), (3, 3));
    }

    #[test]
    fn blob_search_forward_first() {
        let data = b"xxabcxxabcxx";
        let (hit, off) = run_search("abc", data, true);
        assert_eq!(hit, Some((2, 3)));
        assert_eq!(off, 0);
    }

    #[test]
    fn blob_search_forward_skip_self_and_wrap() {
        let data = b"xxabcxxabcxx";
        let mut st = DetailState::default();
        st.val.blob_input = "abc".into();
        // 第一次：位置 2
        let _ = st.blob_search(Side::Value, data, true);
        assert_eq!(st.val.blob_hit, Some((2, 3)));
        // 第二次：位置 7
        let _ = st.blob_search(Side::Value, data, true);
        assert_eq!(st.val.blob_hit, Some((7, 3)));
        // 第三次：回绕到 2
        let (_, msg) = st.blob_search(Side::Value, data, true);
        assert_eq!(st.val.blob_hit, Some((2, 3)));
        assert!(msg.contains("0x2"));
    }

    #[test]
    fn blob_search_reports_match_count() {
        // VSCode 风格 n/m：命中后记录（当前序号, 总数）；未命中/空输入时清除
        let mut st = DetailState::default();
        let data = b"ab--ab--ab";
        st.val.blob_input = "ab".into();

        st.blob_search(Side::Value, data, true);
        assert_eq!(st.val.blob_matches, Some((1, 3)));
        st.blob_search(Side::Value, data, true);
        assert_eq!(st.val.blob_matches, Some((2, 3)));
        st.blob_search(Side::Value, data, true);
        st.blob_search(Side::Value, data, true); // 回绕到第 1 个
        assert_eq!(st.val.blob_matches, Some((1, 3)));
        st.blob_search(Side::Value, data, false); // 向前回绕到最后一个
        assert_eq!(st.val.blob_matches, Some((3, 3)));

        // 不重叠计数：aaaa 中搜 aa 应为 2 个
        st.val.blob_input = "aa".into();
        st.blob_search(Side::Value, b"aaaa", true);
        assert_eq!(st.val.blob_matches, Some((1, 2)));

        // 未命中清除计数
        st.val.blob_input = "zz".into();
        st.blob_search(Side::Value, data, true);
        assert_eq!(st.val.blob_matches, None);

        // 空输入清除计数
        st.val.blob_input = String::new();
        st.blob_search(Side::Value, data, true);
        assert_eq!(st.val.blob_matches, None);
    }

    #[test]
    fn blob_search_backward() {
        let data = b"xxabcxxabcxx";
        let mut st = DetailState::default();
        st.val.blob_input = "abc".into();
        // 无命中锚点：全文反向找最后一个
        let _ = st.blob_search(Side::Value, data, false);
        assert_eq!(st.val.blob_hit, Some((7, 3)));
        // 继续向前：位置 2
        let _ = st.blob_search(Side::Value, data, false);
        assert_eq!(st.val.blob_hit, Some((2, 3)));
        // 再向前：回绕到 7
        let _ = st.blob_search(Side::Value, data, false);
        assert_eq!(st.val.blob_hit, Some((7, 3)));
    }

    #[test]
    fn blob_search_hex_input() {
        let data = [0x00, 0x11, 0xDE, 0xAD, 0xBE, 0xEF, 0x22];
        let (hit, _) = run_search("hex(deadbeef)", &data, true);
        assert_eq!(hit, Some((2, 4)));
    }

    #[test]
    fn blob_search_no_match() {
        let data = b"hello world";
        let (hit, _) = run_search("zzz", data, true);
        assert_eq!(hit, None);
    }

    #[test]
    fn blob_search_segments_aligned() {
        // 命中落在第 2 个 64KiB 段：段偏移对齐到 PAGE_BYTES
        let mut data = vec![0u8; crate::fmt::PAGE_BYTES + 100];
        let p = crate::fmt::PAGE_BYTES + 10;
        data[p..p + 3].copy_from_slice(b"zzz");
        let (hit, off) = run_search("zzz", &data, true);
        assert_eq!(hit, Some((p, 3)));
        assert_eq!(off, crate::fmt::PAGE_BYTES);
    }

    #[test]
    fn blob_search_empty_prompt() {
        let mut st = DetailState::default();
        let (_, msg) = st.blob_search(Side::Value, b"abc", true);
        assert!(!msg.is_empty());
        assert_eq!(st.val.blob_hit, None);
    }
}
