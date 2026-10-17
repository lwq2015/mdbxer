// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 压缩容器识别与解压：zstd / gzip / zlib / lz4（frame）。
//!
//! MDBX 不提供内置压缩，应用常自行压缩后再存值；这里靠魔数识别
//! （误判率≈0），解压后交回 Auto 猜测链递归解码，实现"套娃数据"穿透。
//! 解压输出上限 [`MAX_DECOMP`]，防解压炸弹拖垮 UI。

use std::io::Read;

/// 单次解压输出的字节上限（64 MiB）：超出按解压失败处理。
pub(crate) const MAX_DECOMP: usize = 64 * 1024 * 1024;

/// 读取并限制输出大小：take(MAX+1) 读满即超限（真 EOF 时不会到 MAX+1）。
fn read_limited<R: Read>(r: R) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    r.take((MAX_DECOMP + 1) as u64).read_to_end(&mut out).ok()?;
    (out.len() <= MAX_DECOMP).then_some(out)
}

/// 按魔数识别压缩容器并解压。返回 (算法名, 解压字节)；
/// 无魔数 / 解压失败 / 超上限返回 None。
pub(crate) fn decompress(bytes: &[u8]) -> Option<(&'static str, Vec<u8>)> {
    // zstd：28 B5 2F FD（frame magic）
    if bytes.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
        let dec = zstd::stream::Decoder::new(std::io::Cursor::new(bytes)).ok()?;
        return read_limited(dec).map(|v| ("zstd", v));
    }

    // gzip：1F 8B
    if bytes.starts_with(&[0x1F, 0x8B]) {
        let dec = flate2::read::GzDecoder::new(bytes);
        return read_limited(dec).map(|v| ("gzip", v));
    }

    // zlib：CMF 0x78（deflate + 32K 窗口）且 (CMF<<8|FLG) % 31 == 0（校验头）
    if bytes.len() >= 2
        && bytes[0] == 0x78
        && u16::from_be_bytes([bytes[0], bytes[1]]) % 31 == 0
    {
        let dec = flate2::read::ZlibDecoder::new(bytes);
        return read_limited(dec).map(|v| ("zlib", v));
    }

    // LZ4 frame：04 22 4D 18
    if bytes.starts_with(&[0x04, 0x22, 0x4D, 0x18]) {
        let dec = lz4_flex::frame::FrameDecoder::new(bytes);
        return read_limited(dec).map(|v| ("lz4", v));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn gz(data: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    fn zlb(data: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    #[test]
    fn decompress_all_formats() {
        let json: &[u8] = br#"{"a":1}"#;

        // zstd
        let z = zstd::stream::encode_all(json, 3).unwrap();
        let (algo, out) = decompress(&z).unwrap();
        assert_eq!(algo, "zstd");
        assert_eq!(out, json);

        // gzip
        let (algo, out) = decompress(&gz(json)).unwrap();
        assert_eq!(algo, "gzip");
        assert_eq!(out, json);

        // zlib
        let (algo, out) = decompress(&zlb(json)).unwrap();
        assert_eq!(algo, "zlib");
        assert_eq!(out, json);

        // lz4 frame
        let mut e = lz4_flex::frame::FrameEncoder::new(Vec::new());
        e.write_all(json).unwrap();
        let l = e.finish().unwrap();
        let (algo, out) = decompress(&l).unwrap();
        assert_eq!(algo, "lz4");
        assert_eq!(out, json);

        // 压缩但损坏/截断的数据 → None
        assert!(decompress(&z[..z.len() - 2]).is_none());
        assert!(decompress(&gz(json)[..3]).is_none());

        // 非压缩数据
        assert!(decompress(json).is_none());
        assert!(decompress(b"").is_none());
    }
}
