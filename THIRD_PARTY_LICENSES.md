# Third-party dependencies

The locked graph contains only these three crates. They are fetched by Cargo;
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
