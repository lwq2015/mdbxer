// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! DB 层：仅依赖 libmdbx + std，封装所有 MDBX 访问，对上层暴露自有纯数据结构。

mod handle;
mod page;
mod stats;

pub use handle::{DbHandle, TableInfo};
pub use page::{
    Anchor, Direction, JumpKey, RawAnchor, RawBatch, Row, dup_find, dups_of, fetch_page_prefix,
    fetch_raw_batch, jump_to,
};
pub use stats::{env_info_view, table_stat_view};

/// 数据库打开方式（对应 MDBX_NOSUBDIR）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum OpenMode {
    /// 按路径自动识别：文件 → 单文件，目录 → 子目录
    #[default]
    Auto,
    /// 路径即数据文件本身
    SingleFile,
    /// 路径为包含 mdbx.dat 的目录
    Directory,
}

impl OpenMode {
    pub const ALL: [OpenMode; 3] = [OpenMode::Auto, OpenMode::SingleFile, OpenMode::Directory];

    /// 下拉框显示文本（随界面语言）。
    pub fn label(self) -> String {
        let t = crate::i18n::tr();
        match self {
            OpenMode::Auto => t.m_auto.to_string(),
            OpenMode::SingleFile => t.m_file.to_string(),
            OpenMode::Directory => t.m_dir.to_string(),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            OpenMode::Auto => "auto",
            OpenMode::SingleFile => "file",
            OpenMode::Directory => "dir",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "file" => OpenMode::SingleFile,
            "dir" => OpenMode::Directory,
            _ => OpenMode::Auto,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_mode_as_str_from_str_round_trip() {
        for m in OpenMode::ALL {
            assert_eq!(OpenMode::from_str(m.as_str()), m);
        }
    }

    #[test]
    fn open_mode_from_str_unknown_is_auto() {
        assert_eq!(OpenMode::from_str(""), OpenMode::Auto);
        assert_eq!(OpenMode::from_str("xyz"), OpenMode::Auto);
    }

    #[test]
    fn open_mode_default_is_auto() {
        assert_eq!(OpenMode::default(), OpenMode::Auto);
    }

    #[test]
    fn open_mode_all_has_three() {
        assert_eq!(OpenMode::ALL.len(), 3);
    }

    #[test]
    fn open_mode_label_follows_lang() {
        let prev = crate::i18n::lang();
        crate::i18n::set_lang(crate::i18n::Lang::Zh);
        let t = crate::i18n::tr();
        assert_eq!(OpenMode::Auto.label(), t.m_auto);
        assert_eq!(OpenMode::SingleFile.label(), t.m_file);
        assert_eq!(OpenMode::Directory.label(), t.m_dir);
        crate::i18n::set_lang(prev);
    }
}
