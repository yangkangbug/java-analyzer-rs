# Third-party dependencies

The locked dependency graph is listed below. Crates are fetched by Cargo;
this source archive does not vendor dependency code or dependency binaries.
It does include the hand-assembled `tests/fixtures/classes/Lib.class` test fixture.
The analysis core does not invoke a JVM.

## cafebabe 0.9.0

- Author: Kartikaya Gupta.
- SPDX license declared by the published Cargo metadata: `0BSD`.
- Source: https://github.com/staktrace/cafebabe
- Package source commit: `9e09acd82a855fec816e8b61c3a902d20e40cb17`.
- Neither the published crate nor that source tree contains a license-text file.
  `licenses/cafebabe/0BSD-template.txt` is the unchanged standard SPDX template
  from https://github.com/spdx/license-list-data/blob/main/text/0BSD.txt,
  blob `0b8ae762b2405a862d8592c5ff2799a1bd9c43e0`. Its YEAR/AUTHOR/EMAIL fields
  are template placeholders, not a claimed cafebabe copyright statement.

## bitflags 1.3.2

- Copyright (c) 2014 The Rust Project Developers.
- License: `MIT OR Apache-2.0`.
- Both published license texts are retained unchanged in `licenses/bitflags/`.
- Source: https://github.com/bitflags/bitflags

## cesu8 1.1.0

- Copyright 2012-2014 The Rust Project Developers and Eric Kidd.
- Source headers license the code under `MIT OR Apache-2.0`; this distribution
  uses the Apache-2.0 option. The full Apache text is in the project LICENSE.
- `licenses/cesu8/COPYRIGHT-RUST.txt` is copied unchanged from the published
  crate. It is a historical upstream notice, not a list of all components
  used by this project. The upstream README's LICENSE-RUST.txt link is stale.
- Source: https://github.com/emk/cesu8-rs

## Project and IntelliJ-derived decisions

See LICENSE, NOTICE, and PROVENANCE.md. No OpenIDEA UI assets, IntelliJ binaries,
Lombok bundles, or other unrelated OpenIDEA dependencies are included.

## Native JAR reader dependency graph

`zip` is exact-pinned to 9.5.0 because bounded central-directory preflight uses
its documented unstable `ZipIterable` API. Default features are disabled; the
only enabled feature is `deflate-flate2-zlib-rs` (pure-Rust DEFLATE). Encryption,
additional compression formats, and time features are disabled. No extraction
APIs are invoked.
Some upstream dependencies remain unconditional even when their features are off.
The table covers every additional package in Cargo.lock, including dependencies
used only on Windows, macOS/BSD, WASM, and other targets. Published license/notice
texts are preserved unchanged under `licenses/<crate>-<version>/`.
The published r-efi crate has no license-text files: its AUTHORS notice is
preserved, and this distribution chooses its Apache-2.0 option (full text in
the project LICENSE), not its alternative LGPL license.

| Package | Locked version | Declared SPDX license | Upstream source |
| --- | --- | --- | --- |
| bitflags | 2.13.2 | `MIT OR Apache-2.0` | https://github.com/bitflags/bitflags |
| bumpalo | 3.20.3 | `MIT OR Apache-2.0` | https://github.com/fitzgen/bumpalo |
| cfg-if | 1.0.5 | `MIT OR Apache-2.0` | https://github.com/rust-lang/cfg-if |
| cfg_aliases | 0.2.2 | `MIT` | https://github.com/katharostech/cfg_aliases |
| constant_time_eq | 0.6.1 | `CC0-1.0 OR MIT-0 OR Apache-2.0` | https://codeberg.org/cesarb/constant_time_eq |
| crc32fast | 1.5.4 | `MIT OR Apache-2.0` | https://github.com/srijs/rust-crc32fast |
| equivalent | 1.0.3 | `Apache-2.0 OR MIT` | https://github.com/indexmap-rs/equivalent |
| flate2 | 1.1.11 | `MIT OR Apache-2.0` | https://github.com/rust-lang/flate2-rs |
| getrandom | 0.4.3 | `MIT OR Apache-2.0` | https://github.com/rust-random/getrandom |
| hashbrown | 0.17.1 | `MIT OR Apache-2.0` | https://github.com/rust-lang/hashbrown |
| indexmap | 2.14.2 | `Apache-2.0 OR MIT` | https://github.com/indexmap-rs/indexmap |
| js-sys | 0.3.107 | `MIT OR Apache-2.0` | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys |
| libc | 0.2.191 | `MIT OR Apache-2.0` | https://github.com/rust-lang/libc |
| memchr | 2.8.3 | `Unlicense OR MIT` | https://github.com/BurntSushi/memchr |
| nix | 0.31.3 | `MIT` | https://github.com/nix-rust/nix |
| proc-macro2 | 1.0.107 | `MIT OR Apache-2.0` | https://github.com/dtolnay/proc-macro2 |
| quote | 1.0.47 | `MIT OR Apache-2.0` | https://github.com/dtolnay/quote |
| r-efi | 6.0.0 | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | https://github.com/r-efi/r-efi |
| rustversion | 1.0.23 | `MIT OR Apache-2.0` | https://github.com/dtolnay/rustversion |
| syn | 2.0.119 | `MIT OR Apache-2.0` | https://github.com/dtolnay/syn |
| syn | 3.0.9 | `MIT OR Apache-2.0` | https://github.com/dtolnay/syn |
| typed-path | 0.13.0 | `MIT OR Apache-2.0` | https://github.com/chipsenkbeil/typed-path |
| unicode-ident | 1.0.27 | `(MIT OR Apache-2.0) AND Unicode-3.0` | https://github.com/dtolnay/unicode-ident |
| wasm-bindgen | 0.2.130 | `MIT OR Apache-2.0` | https://github.com/wasm-bindgen/wasm-bindgen |
| wasm-bindgen-macro | 0.2.130 | `MIT OR Apache-2.0` | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro |
| wasm-bindgen-macro-support | 0.2.130 | `MIT OR Apache-2.0` | https://github.com/wasm-bindgen/wasm-bindgen/tree/main/crates/macro-support |
| wasm-bindgen-shared | 0.2.130 | `MIT OR Apache-2.0` | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared |
| windows-link | 0.2.1 | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| windows-sys | 0.61.2 | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| zeroize | 1.9.1 | `Apache-2.0 OR MIT` | https://github.com/RustCrypto/utils |
| zeroize_derive | 1.5.0 | `Apache-2.0 OR MIT` | https://github.com/RustCrypto/utils |
| zip | 9.5.0 | `MIT` | https://github.com/zip-rs/zip2 |
| zlib-rs | 0.6.8 | `Zlib` | https://github.com/trifectatechfoundation/zlib-rs |
