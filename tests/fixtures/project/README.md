# Source-to-JAR project fixture

Two Java source files inherit `lib.Lib.run()` from `deps/library.jar`. The JAR
contains exactly the existing hand-assembled `../classes/Lib.class` fixture.
No compiler or JVM is required. It is a small acceptance fixture, not evidence
of full Java or real-project performance coverage.

Reproduce the archive from the repository root with Python's standard library:

```python
from pathlib import Path
from zipfile import ZipFile, ZipInfo, ZIP_DEFLATED
entry = ZipInfo('lib/Lib.class', date_time=(2020, 1, 1, 0, 0, 0))
entry.compress_type = ZIP_DEFLATED
entry.external_attr = 0o100644 << 16
with ZipFile('tests/fixtures/project/deps/library.jar', 'w') as jar:
    jar.writestr(entry, Path('tests/fixtures/classes/Lib.class').read_bytes())
```

Deflate byte output can depend on the zlib version; tests verify decompressed
class content and behavior, not the compressed byte representation.
