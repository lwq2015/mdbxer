// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 校验和：对 Key/Value 的原始字节计算（不受解码格式、单元格截断与分段影响）。
//! 详情卡片底部的"校验"行用它；哪些算法显示由「高级配置」里的开关决定。

use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256};

/// 支持的校验算法。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HashAlg {
    Md5,
    Sha1,
    Sha256,
}

impl HashAlg {
    /// 全部算法（配置下拉的顺序）。
    pub const ALL: [HashAlg; 3] = [HashAlg::Md5, HashAlg::Sha1, HashAlg::Sha256];

    /// 算法名（三种语言通用，直接用标准写法）。
    pub fn name(self) -> &'static str {
        match self {
            HashAlg::Md5 => "MD5",
            HashAlg::Sha1 => "SHA-1",
            HashAlg::Sha256 => "SHA-256",
        }
    }

    /// 持久化代码（config.json 里用，与语言无关）。
    pub fn code(self) -> &'static str {
        match self {
            HashAlg::Md5 => "md5",
            HashAlg::Sha1 => "sha1",
            HashAlg::Sha256 => "sha256",
        }
    }

    /// 从持久化代码解析；未知值回退 MD5。
    pub fn from_code(s: &str) -> Option<HashAlg> {
        match s {
            "md5" => Some(HashAlg::Md5),
            "sha1" => Some(HashAlg::Sha1),
            "sha256" => Some(HashAlg::Sha256),
            _ => None,
        }
    }

    /// 对字节计算摘要，返回小写十六进制串。
    pub fn hex(self, bytes: &[u8]) -> String {
        match self {
            HashAlg::Md5 => to_hex(Md5::digest(bytes).as_slice()),
            HashAlg::Sha1 => to_hex(Sha1::digest(bytes).as_slice()),
            HashAlg::Sha256 => to_hex(Sha256::digest(bytes).as_slice()),
        }
    }
}

/// 字节序列转小写十六进制。
fn to_hex(d: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(d.len() * 2);
    for b in d {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::HashAlg;

    /// RFC 1321 / FIPS 180 的"abc"与空串标准向量。
    #[test]
    fn known_vectors() {
        assert_eq!(HashAlg::Md5.hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(HashAlg::Md5.hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(
            HashAlg::Sha1.hex(b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            HashAlg::Sha256.hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// 长度固定、全小写、只含 hex 字符。
    #[test]
    fn hex_format_is_canonical() {
        for (alg, len) in [
            (HashAlg::Md5, 32),
            (HashAlg::Sha1, 40),
            (HashAlg::Sha256, 64),
        ] {
            let s = alg.hex(&[0xff; 7]);
            assert_eq!(s.len(), len);
            assert!(
                s.chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            );
        }
    }
}
