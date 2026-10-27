# MDBXer

[简体中文](README.md) | English

MDBXer is a cross-platform GUI viewer and browser for [libmdbx](https://libmdbx.dqdkfa.ru/) databases. Built with Rust and egui, it lets you browse tables, keys, and values, and inspect structured or binary data including JSON, BSON, and Protocol Buffers. Available for Windows, Linux, and macOS.

![MDBXer screenshot](docs/screenshot.png)

## Downloads

Prebuilt releases are available on [GitHub Releases](https://github.com/lwq2015/mdbxer/releases) for Windows x86_64, Linux x86_64, and macOS Apple Silicon. [Gitee Releases](https://gitee.com/lwq_yu/mdbxer/releases) provides a mirror. macOS releases are Apple Silicon only; Intel Macs can run them with Rosetta 2.

## Features

### Opening databases

- **File / directory / automatic** modes for MDBX's `MDBX_NOSUBDIR` layouts.
- Open files through the system file picker, recent-file list, drag and drop, or a command-line path.
- Databases are opened read-only for routine browsing. If a read-only open fails, you can explicitly confirm a read-write recovery attempt, optionally backing up a single-file database first.
- Up to 20 recent paths are kept, deduplicated and sorted by last use, in `%APPDATA%\mdbxer\history.json`.

### Browsing data

- **Anchor-based pagination** avoids a full `COUNT`; page navigation cost is independent of table size. Page sizes: 50, 100, 200, 500, or 1000 entries.
- **DUP_SORT tables are grouped by key**, so a key with many values does not take over a page.
- Click column headers to sort (Key toggles the global direction; other columns sort within the current page). Use `↺` to restore the original order.
- Locate entries with `hex(...)`, `0x...`, text, or decimal input (for `INTEGER_KEY` tables).

### Duplicate-value navigation

- Move between values with `◀ ▶`, between pages of values with `⏪ ⏩` (±100), or jump to the first/last value with `⏮ ⏭`.
- Jump to a value by index or search its contents (text or byte substring, with wraparound).
- Values are loaded lazily, 100 per page.

### Format decoding

35 formats, including automatic detection:

`Auto / utf8 / utf16(LE) / utf16(BE) / gb18030 / windows-1251 / shift_jis / big5 / windows-1252 / int8 / int16 / int32 / int64 / uint8 / uint16 / uint32 / uint64 / float / double / hex / dec / binary / base64 / uuid / mac / ip / time / json / bson / msgpack / cbor / proto / yaml / toml / xml`

- **Automatic detection:** decompresses one layer of zstd, gzip, zlib, LZ4, bzip2, xz, Snappy, or Brotli, then detects the inner format (for example, `gzip → JSON`). It also recognizes JSON, BSON, MessagePack, CBOR, XML, YAML, TOML, printable UTF-8, Protobuf when a schema is loaded, UTF-16, integers, and binary data.
- Recognizes common file signatures (PNG, JPEG, GIF, WebP, WAV, ICO, PDF, ZIP, RAR, 7z, FLAC, Ogg, MP3, MP4, and SQLite). In table cells, Auto also interprets 4-, 6-, and 16-byte values as IPv4, MAC, and IPv6 addresses.
- Values of `uint64` size that fall within Unix timestamp or FILETIME ranges include a local-time interpretation. The manual `time` format also supports 4-byte Unix seconds.
- Manual formats cover common encodings (`gb18030`, `windows-1251`, `shift_jis`, `big5`, `windows-1252`), Base64, UUID (16 bytes), MAC (6 bytes), IP (4-byte IPv4 / 16-byte IPv6), and structured JSON, BSON, MessagePack, CBOR, YAML, TOML, and XML, as well as dynamic Protobuf decoding.
- **Protobuf:** load `.proto` files using a pure-Rust compiler (no `protoc` required), then decode using the selected message type and display the result as JSON. Schemas are remembered per database and restored when it is reopened; they can also be cleared. Once loaded, Auto can detect message types.
- Toggle little-/big-endian byte order. Short byte sequences are zero-padded and annotated; invalid lengths for address/time formats fall back to hex with an annotation.
- Optional thousands separators and configurable cell-text truncation (64–4096 characters). Format names are sorted by interface language, with Auto pinned to the top.

### Large values

- Values larger than 64 KiB are browsed in segments. Only the current segment is rendered in the text and hex views.
- The hex dump has independent address / HEX / ASCII toggles and row widths of 4, 8, or 16 bytes.

### Statistics and environment information

- Table statistics: entry count, B+ tree depth, branch/leaf/overflow page counts, page size, total size, and table flags.
- Environment information: file geometry, map size, page count, transaction ID, reader slots, and more.

### Languages

- Chinese, English, and Russian. Switch languages from the top bar; changes take effect immediately.
- The first launch guesses a language from the system locale. The selection is saved in `config.json`.

### Other

- Copy the complete decoded Key/Value text or save the original bytes without truncation or decoding.
- The three columns have constrained widths (left column: 120–320 px; the center area has a 300 px minimum).
- Toolbar dropdowns support mouse-wheel selection.

## Comparison with other MDBX viewers

This comparison focuses on publicly verifiable information about [MDBX Navigator](https://github.com/wmitsuda/mdbx-navigator). It does not infer that an unlisted feature is unsupported, and it is not an exhaustive list of similar projects.

| Aspect | MDBXer | MDBX Navigator |
|---|---|---|
| Interaction | Desktop GUI for Windows, Linux, and macOS | Local backend serves a web UI accessed in a browser |
| Opening a database | File, directory, or automatic detection; supports a file picker, drag and drop, and command-line paths | Specify the database file path when starting the backend |
| Open mode | Read-only for routine browsing; user-confirmed recovery can be attempted if read-only opening fails | README describes read-only, exclusive opening |
| Browsing focus | Table browsing, anchor-based pagination, duplicate-value navigation, and large-value segmentation | Lightweight UI focused on read-only browsing of libmdbx contents |
| Inspecting values | Automatic or manual decoding across 35 formats, including compressed data, JSON/BSON/Protobuf, and hex | Public README primarily describes browsing database tables through the browser UI |
| Distribution and build | Prebuilt Windows, Linux, and macOS releases; source build uses Rust | README describes a Go backend with an embedded web UI; source builds also involve Node.js and pnpm |

**Choosing between them:** MDBXer may suit users who prefer a desktop app and need to inspect encoded, structured, or binary values. MDBX Navigator may suit users who prefer a local, browser-based read-only interface. Check each project's latest documentation and releases for current capabilities and platform support.

## Build and run

Install the Rust toolchain (edition 2024; Rust 1.85 or newer recommended).

```bash
# Run in development
cargo run

# Build a release binary (no console window on Windows)
cargo build --release

# Run unit tests
cargo test
```

Build output: `target/release/mdbxer` on Linux or `target\release\mdbxer.exe` on Windows.

> **Note:** `mdbx-sys` requires native C headers for the target platform and does not support cross-compilation. Build Linux binaries in a Linux environment.

> **Windows note:** `.cargo/config.toml` enables static MSVC runtime linking with `+crt-static`. The binary does not depend on VC runtime DLLs such as `vcruntime140.dll` or `ucrtbase.dll` and can run on Windows 10/11 without installing the Visual C++ Redistributable.

## Test data

Run `examples/make_test_db.rs` to generate comprehensive test databases (`testdata/dir_db` and `testdata/file_db.mdbx`). They include regular, duplicate-value, and `INTEGER_KEY` tables; large fields; numeric and boundary values; JSON/BSON/MessagePack/CBOR samples detectable by Auto; eight compressed containers (zstd, gzip, zlib, LZ4 frame, bzip2, xz, Snappy frame, and Brotli); address/timestamp and file-signature samples; and Protobuf samples.

The `json_search_long` sample contains 96 records. Search for `needle-amber` to test multiple matches, highlight-all, and previous/next navigation. Each run deletes and recreates both database outputs so stale tables and deleted samples do not remain. If a database is open in MDBXer, or cleanup/writing fails, the command exits with an error instead of reporting success.

- The script generates `testdata/test.proto`, covering all 15 scalar types (varint, zigzag, and fixed-width integer encodings), enums, repeated fields, nested messages, maps with three value types, oneofs, and optional fields.
- Samples `proto_simple`, `proto_mixed`, `proto_scalars`, `proto_shapes`, and `proto_tables` are encoded dynamically through the same pipeline used by the UI, keeping wire data consistent with the schema.
- To inspect them, switch to Protobuf formatting, select **Load .proto**, open `testdata/test.proto`, and choose a `mdbxer.test.*` message type. Alternatively, Auto can detect the type after the schema is loaded.

```bash
cargo run --example make_test_db
```

## License

[Apache-2.0](LICENSE)

This project statically links libmdbx (OpenLDAP Public License 2.8). See [NOTICE](NOTICE) for the applicable notices.

---

## Disclaimer

1. **Routine browsing is read-only.** The application opens databases with `Mode::ReadOnly` and `accede` and does not write to them during routine browsing. If read-only opening fails, you may explicitly confirm a recovery attempt. This temporarily opens the database read-write and may modify files. For single-file databases, you can choose to create a `.bak-recover` backup first. Back up important data before use.

2. **Provided “as is.”** This software is distributed under the terms of the Apache License 2.0, without warranties of any kind, express or implied, including merchantability, fitness for a particular purpose, and non-infringement. In no event shall the authors or copyright holders be liable for claims, damages, or other liability arising from use of the software.

3. **Data safety.** Even when this application opens a database read-only, files may be inconsistent if another process is writing to them. A user-confirmed recovery operation may also modify files. The project is not responsible for data integrity, parsing accuracy, or decisions based on the displayed output.

4. **Use at your own risk.** You assume all risks associated with using this software. The authors accept no liability for direct or indirect losses resulting from its use, including data loss, business interruption, or lost profits.

5. **Third-party components.** This software depends on libmdbx and other third-party components. Their respective license terms and disclaimers apply. Retain [LICENSE](LICENSE) and [NOTICE](NOTICE) when redistributing the software.
