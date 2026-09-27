//! DB 层：仅依赖 libmdbx + std，封装所有 MDBX 访问，对上层暴露自有纯数据结构。

mod handle;
mod page;
mod stats;

pub use handle::{DbHandle, TableInfo};
pub use page::{dups_of, dup_index_of, fetch_page, jump_to, Anchor, Direction, JumpKey, Row};
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

    pub fn label(self) -> &'static str {
        match self {
            OpenMode::Auto => "自动",
            OpenMode::SingleFile => "单文件",
            OpenMode::Directory => "目录",
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
