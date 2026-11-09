# Source and modification provenance

Extracted on 2026-11-09 from `crates/java-engine` in:

- Source: https://github.com/yangkangbug/openidea
- Existing PR: https://github.com/yangkangbug/openidea/pull/1
- Source HEAD: `74abe11aec055635c8fec1c60e8861892b7f90f9`
- OpenIDEA fork base: `fca9d1e08faab6e686fc6018eb5e10a37c0ed8bb`
- Upstream: https://github.com/l0ng-ai/openidea

The original crate's six commits are retained as source references, rather than
claiming this new local repository contains their history:

1. `1546960b54e9e04d9339b5aef957bf410f7160a7`: standalone syntax kernel.
2. `fec25673873710417cb0789859ad3382a2d1ad0e`: expression nodes and edit path.
3. `79e862519b532f2ad467b353c7a2948b29226826`: standalone package metadata.
4. `49ea4c32a690baab807ad4954f52ed71f06dd32b`: ranges and statement recovery.
5. `0bac225cb60d24bff54a6294aa4037e9376b21aa`: source binding and resources.
6. `74abe11aec055635c8fec1c60e8861892b7f90f9`: class signatures and supertypes.

## Changes made before extraction

- Source-field receiver binding uses the declaration's file and type context,
  while access checks stay at the caller. Ten regression tests were added.
- Source method shapes preserve array dimensions and normalize varargs to one
  extra array dimension. Ordinary postfix parameter dimensions are parsed.
  Malformed parameter lists are not used as confident signatures. Eleven
  regression tests were added. This is not overload selection or generic erasure.

The extracted `src/` and `tests/` are byte-identical to the modified OpenIDEA
copy at extraction. `Cargo.toml`, `Cargo.lock`, `LICENSE`, the toolchain, and
format settings are retained. New project documentation, notices, example,
and root CI configuration make the copied crate self-contained.

The original OpenIDEA checkout and its uncommitted changes were not removed,
reset, rebased, or pushed. The extraction initially had no remote configured. GitHub creation and pushing
are separate actions and require an explicit destination and visibility choice.

See NOTICE for the precise IntelliJ reimplementation boundaries and licenses.
