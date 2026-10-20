// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 压缩容器识别与解压：zstd / gzip / zlib / lz4（frame）/ bzip2 / xz /
//! snappy（frame）/ brotli。
//!
//! MDBX 不提供内置压缩，应用常自行压缩后再存值；这里靠魔数识别
//! （误判率≈0），解压后交回 Auto 猜测链递归解码，实现"套娃数据"穿透。
//! 解压输出上限 [`MAX_DECOMP`]，防解压炸弹拖垮 UI。

use std::io::{Read, Write};

/// 单次解压输出的字节上限（64 MiB）：超出按解压失败处理。
pub(crate) const MAX_DECOMP: usize = 64 * 1024 * 1024;

/// 读取并限制输出大小：take(MAX+1) 读满即超限（真 EOF 时不会到 MAX+1）。
fn read_limited<R: Read>(r: R) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    r.take((MAX_DECOMP + 1) as u64).read_to_end(&mut out).ok()?;
    (out.len() <= MAX_DECOMP).then_some(out)
}

/// 按魔数识别压缩容器并解压（brotli 无魔数，放最后试探解码兜底）。
/// 返回 (算法名, 解压字节)；无魔数 / 解压失败 / 超上限返回 None。
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
    if bytes.len() >= 2 && bytes[0] == 0x78 && u16::from_be_bytes([bytes[0], bytes[1]]) % 31 == 0 {
        let dec = flate2::read::ZlibDecoder::new(bytes);
        return read_limited(dec).map(|v| ("zlib", v));
    }

    // LZ4 frame：04 22 4D 18
    if bytes.starts_with(&[0x04, 0x22, 0x4D, 0x18]) {
        let dec = lz4_flex::frame::FrameDecoder::new(bytes);
        return read_limited(dec).map(|v| ("lz4", v));
    }

    // bzip2："BZh" + 层级数字 '1'..'9'
    if matches!(bytes, [b'B', b'Z', b'h', b'1'..=b'9', ..]) {
        let dec = bzip2::read::BzDecoder::new(bytes);
        return read_limited(dec).map(|v| ("bzip2", v));
    }

    // xz：FD 37 7A 58 5A 00
    if bytes.starts_with(&[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00]) {
        let mut out = Vec::new();
        let mut w = LimitedWriter {
            cap: MAX_DECOMP,
            out: &mut out,
        };
        lzma_rs::xz_decompress(&mut std::io::Cursor::new(bytes), &mut w).ok()?;
        return (out.len() <= MAX_DECOMP).then_some(("xz", out));
    }

    // snappy framed 流：FF 06 00 00 "sNaPpY"（原始块格式无魔数，不识别）
    if bytes.starts_with(&[0xFF, 0x06, 0x00, 0x00, b's', b'N', b'a', b'P', b'p', b'Y']) {
        let dec = snap::read::FrameDecoder::new(bytes);
        return read_limited(dec).map(|v| ("snappy", v));
    }

    // brotli：无魔数，只能试探解码，故放全部魔数识别之后兜底。短数据
    // （<16 字节）有与小整数误撞的风险（单字节 0x03 即合法空流），跳过不解；
    // 解压失败或输出为空按未命中处理。
    if bytes.len() >= 16 {
        if let Some(out) = read_limited(brotli_decompressor::Decompressor::new(bytes, 4096)) {
            if !out.is_empty() {
                return Some(("brotli", out));
            }
        }
    }

    None
}

/// 带上限的 Write：超出 cap 即报错（配合 lzma_rs 一体式 API 限制输出）。
struct LimitedWriter<'a> {
    cap: usize,
    out: &'a mut Vec<u8>,
}

impl Write for LimitedWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.out.len() + buf.len() > self.cap {
            return Err(std::io::Error::new(
                std::io::ErrorKind::OutOfMemory,
                "decompressed data exceeds limit",
            ));
        }

        self.out.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
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

        // bzip2
        let mut e = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
        e.write_all(json).unwrap();
        let bz = e.finish().unwrap();
        let (algo, out) = decompress(&bz).unwrap();
        assert_eq!(algo, "bzip2");
        assert_eq!(out, json);

        // xz
        let mut xz = Vec::new();
        lzma_rs::xz_compress(&mut std::io::Cursor::new(json), &mut xz).unwrap();
        let (algo, out) = decompress(&xz).unwrap();
        assert_eq!(algo, "xz");
        assert_eq!(out, json);

        // 压缩但损坏/截断的数据 → None
        assert!(decompress(&z[..z.len() - 2]).is_none());
        assert!(decompress(&gz(json)[..3]).is_none());
        assert!(decompress(&bz[..bz.len() - 4]).is_none());
        assert!(decompress(&xz[..xz.len() - 4]).is_none());

        // 非压缩数据
        assert!(decompress(json).is_none());
        assert!(decompress(b"").is_none());
    }

    #[test]
    fn decompress_snappy_and_brotli() {
        let json: &[u8] = br#"{"a":1}"#;

        // snappy framed
        let mut e = snap::write::FrameEncoder::new(Vec::new());
        e.write_all(json).unwrap();
        let sn = e.into_inner().unwrap();
        let (algo, out) = decompress(&sn).unwrap();
        assert_eq!(algo, "snappy");
        assert_eq!(out, json);
        // 截断 → None
        assert!(decompress(&sn[..sn.len() - 2]).is_none());

        // brotli（样本用完整 brotli crate 生成，运行时只依赖 decompressor）。
        // 样本取长一点：短数据压缩后不足 16 字节，会被试探门槛跳过。
        let long: &[u8] = br#"{"a":1,"b":"hello world","n":[1,2,3,4,5,6,7,8]}"#;
        let mut e = brotli::CompressorWriter::new(Vec::new(), 4096, 5, 22);
        e.write_all(long).unwrap();
        let br = e.into_inner();
        assert!(br.len() >= 16, "brotli 样本应长于试探门槛: {}", br.len());
        let (algo, out) = decompress(&br).unwrap();
        assert_eq!(algo, "brotli");
        assert_eq!(out, long);
        // 截断 → None
        assert!(decompress(&br[..br.len() - 2]).is_none());

        // 短数据不试探 brotli：0x03 是合法空流，但太短不解（避免与小整数误撞）
        assert!(decompress(&[0x03]).is_none());
        assert!(decompress(&[0x03, 0x00, 0x00, 0x00]).is_none());

        // 非压缩的较长文本不误判
        assert!(decompress(b"hello, world, longer!").is_none());
    }
}
