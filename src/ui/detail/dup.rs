// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 多值（DUP_SORT）导航：分页懒加载、序号跳转、值内容搜索。

use super::state::{DUP_PAGE_SIZE, DetailState};
use crate::db;
use crate::i18n::tr;
use crate::ui::{MsgLevel, parse_bytes_input};
use libmdbx::{Database, NoWriteMap};

/// 多值操作需要的数据库上下文：库句柄 + 表名（None = 主表）+ 当前 Key。
pub struct DupCtx<'a> {
    /// 数据库句柄（只读）
    pub db: &'a Database<NoWriteMap>,
    /// 表名；None = 主表
    pub table: Option<&'a str>,
    /// 当前选中行的 Key 字节
    pub key: &'a [u8],
}

impl DetailState {
    /// 加载指定 Key 的多值首页（值列表每页 DUP_PAGE_SIZE 个懒加载）；
    /// 非多值表只清空状态。
    ///
    /// - `table`：None = 主表
    /// - `dup_sort`：该表是否为 DUP_SORT 多值表
    /// - `key`：选中行的 Key 字节
    pub fn load_dups(
        &mut self,
        db: &Database<NoWriteMap>,
        table: Option<&str>,
        dup_sort: bool,
        key: &[u8],
    ) {
        self.clear();
        if !dup_sort {
            return;
        }
        if let Ok((total, values)) = db::dups_of(db, table, key, 0, DUP_PAGE_SIZE) {
            self.dup.total = total.max(1);
            self.dup.values = values;
        }
    }

    /// 跳转到当前 Key 的第 `idx` 个值（0 起）；自动夹到有效范围，跨值页时懒加载。
    ///
    /// 返回 `Ok(())` 表示成功（或目标已在当前页，无需 IO）；
    /// 返回 `Err(msg)` 表示跨页加载失败，此时 `dup.index` 保持不变，
    /// 调用者应将 `msg` 显示到状态栏。
    pub fn dup_goto(&mut self, ctx: &DupCtx, idx: usize) -> Result<(), String> {
        if self.dup.total == 0 {
            return Ok(());
        }

        let idx = idx.min(self.dup.total - 1);
        let in_page =
            idx >= self.dup.page_start && idx < self.dup.page_start + self.dup.values.len();
        if !in_page {
            let page_index = idx / DUP_PAGE_SIZE;
            match db::dups_of(ctx.db, ctx.table, ctx.key, page_index, DUP_PAGE_SIZE) {
                Ok((total, values)) => {
                    self.dup.total = total.max(1);
                    self.dup.page_start = page_index * DUP_PAGE_SIZE;
                    self.dup.values = values;
                }
                Err(e) => return Err(tr().dup_load_fail(&e)),
            }
        }

        self.dup.index = idx.min(self.dup.total.saturating_sub(1));

        // 切换到另一个值：Value 分段偏移归零；旧值的搜索命中失效（输入保留）
        self.val.reset_transient();
        Ok(())
    }

    /// 上/下一个值（边界停止）。跨页加载失败时返回错误消息。
    pub fn dup_step(&mut self, ctx: &DupCtx, delta: isize) -> Result<(), String> {
        let new = self.dup.index as isize + delta;
        if new < 0 || new >= self.dup.total as isize {
            return Ok(());
        }
        self.dup_goto(ctx, new as usize)
    }

    /// 上/下翻一个值页（DUP_PAGE_SIZE 个值），到头自动夹住。跨页加载失败时返回错误消息。
    pub fn dup_page_step(&mut self, ctx: &DupCtx, pages: isize) -> Result<(), String> {
        let target = self.dup.index as isize + pages * DUP_PAGE_SIZE as isize;
        if target < 0 {
            self.dup_goto(ctx, 0)
        } else {
            self.dup_goto(ctx, target as usize)
        }
    }

    /// 序号跳转：输入为 1 起的十进制序号。返回（级别, 状态栏消息）。
    pub fn dup_jump(&mut self, ctx: &DupCtx) -> (MsgLevel, String) {
        let t = tr();
        let s = self.dup.jump_input.trim();
        match s.parse::<usize>() {
            Ok(n) if n >= 1 && n <= self.dup.total => match self.dup_goto(ctx, n - 1) {
                Ok(()) => (MsgLevel::Info, t.dup_located(n, self.dup.total)),
                Err(e) => (MsgLevel::Error, e),
            },
            Ok(n) => (MsgLevel::Warn, t.dup_range(n, self.dup.total)),
            Err(_) => (MsgLevel::Warn, t.dup_bad_num.to_string()),
        }
    }

    /// 在当前 Key 的值中按内容搜索：文本按 UTF-8，hex(...)/0x... 按字节；字节子串匹配。
    /// `forward=false` 向小序号方向查找；主方向无命中时回绕。返回（级别, 状态栏消息）。
    pub fn dup_search(&mut self, ctx: &DupCtx, forward: bool) -> (MsgLevel, String) {
        let t = tr();
        let s = self.dup.search_input.trim();
        if s.is_empty() {
            return (MsgLevel::Warn, t.dup_prompt.to_string());
        }

        let needle = match parse_bytes_input(s) {
            Ok(b) => b,
            Err(e) => return (MsgLevel::Error, t.dup_bad_query(&e)),
        };

        // 向后从下一个值开始；向前从当前值之前开始
        let from = if forward {
            self.dup.index + 1
        } else {
            self.dup.index
        };

        match db::dup_find(ctx.db, ctx.table, ctx.key, &needle, from, forward) {
            Ok(Some((i, _))) => {
                let wrapped = forward && i < from || !forward && i >= from;
                match self.dup_goto(ctx, i) {
                    Ok(()) => {
                        if wrapped {
                            (MsgLevel::Info, t.dup_wrap(i + 1, self.dup.total))
                        } else {
                            (MsgLevel::Info, t.dup_located(i + 1, self.dup.total))
                        }
                    }
                    Err(e) => (MsgLevel::Error, e),
                }
            }
            Ok(None) => (MsgLevel::Warn, t.dup_nomatch.to_string()),
            Err(e) => (MsgLevel::Error, t.dup_search_fail(&e)),
        }
    }

    /// 当前多值页中当前值的数据（未加载时返回 None）。
    pub fn current_dup_value(&self) -> Option<&Vec<u8>> {
        self.dup
            .values
            .get(self.dup.index.saturating_sub(self.dup.page_start))
    }
}
