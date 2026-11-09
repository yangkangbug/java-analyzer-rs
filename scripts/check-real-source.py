#!/usr/bin/env python3
"""Optional offline acceptance. Archives are data; no Java or build tools run."""
import argparse
import hashlib
from pathlib import Path, PurePosixPath
import subprocess
import tempfile
import zipfile

SOURCES_SHA256 = "5fdcac21ad329766054a95367d7583dfcdca737d221d5e01a5f2a198c04c6b18"
PLATFORM_SHA256 = "fad51abeafe6a1ecb0c2696a88684dc417cc534b74b5d865042df7381d9d0e91"


def verify(path, expected):
    with path.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    if digest != expected:
        raise SystemExit(f"SHA-256 mismatch for {path}: {digest}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sources_jar", type=Path)
    parser.add_argument("java_base_jmod", type=Path)
    parser.add_argument("--profile", choices=("debug", "release"), default="release")
    parser.add_argument("--rounds", type=int, default=5)
    args = parser.parse_args()
    if not 1 <= args.rounds <= 100:
        parser.error("rounds must be between 1 and 100")
    source = args.sources_jar.resolve(strict=True)
    platform = args.java_base_jmod.resolve(strict=True)
    verify(source, SOURCES_SHA256)
    verify(platform, PLATFORM_SHA256)
    root = Path(__file__).resolve().parent.parent
    executable = root / "target" / args.profile / "examples" / "real_source"
    if not executable.is_file():
        raise SystemExit(f"Build the {args.profile} real_source Rust example first: {executable}")
    with tempfile.TemporaryDirectory(prefix="java-engine-real-source-") as temporary:
        destination = Path(temporary)
        with zipfile.ZipFile(source) as archive:
            files = [entry for entry in archive.infolist() if entry.filename.endswith(".java")]
            if len(files) != 249 or sum(entry.file_size for entry in files) != 3676819:
                raise SystemExit("unexpected source inventory")
            seen = set()
            for entry in files:
                relative = PurePosixPath(entry.filename)
                if relative.is_absolute() or ".." in relative.parts or entry.filename in seen:
                    raise SystemExit("unsafe or duplicate source path")
                seen.add(entry.filename)
                target = destination.joinpath(*relative.parts)
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(archive.read(entry))
        print(f"source_sha256={SOURCES_SHA256}\nplatform_sha256={PLATFORM_SHA256}", flush=True)
        # Only the already-built Rust analyzer runs in this empty environment.
        subprocess.run([str(executable), str(destination), str(platform), str(args.rounds)],
                       check=True, env={"PATH": "/nonexistent"})


if __name__ == "__main__":
    main()
