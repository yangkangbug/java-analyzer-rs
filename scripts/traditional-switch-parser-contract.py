"""Exact additive parser stage; historical artifacts and failed predictions remain unchanged."""
import copy
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "examples/traditional_switch"
MANIFEST_SHA = "6173cf70feffad8e4e78e43a2cfe3f045760d0b8c72ecc6dc5daa6cd0d747250"

def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False)

def require(ok, message):
    if not ok:
        raise AssertionError(message)

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def load(corpus, stage="candidate"):
    require(corpus in ("commons", "gson"), "unknown switch corpus")
    require(stage in ("baseline", "candidate"), "unknown switch parser stage")
    require(digest(CORPUS / "manifest.json") == MANIFEST_SHA, "frozen switch manifest changed")
    manifest = json.loads((CORPUS / "manifest.json").read_text())
    for name, checksum in manifest.items():
        require(digest(CORPUS / name) == checksum, "frozen switch artifact changed: " + name)
    read = lambda suffix: json.loads((CORPUS / (corpus + "-" + suffix + ".json")).read_text())
    old, prediction, actual = read("baseline-parser"), read("predicted-parser"), read("candidate-parser")
    require(read("baseline-source-sha256") == read("candidate-source-sha256"), "switch source bytes changed")
    # Reconstruct the original prediction from the complete old output. No generic
    # message filtering occurs against actual output; it must equal the full result.
    derived = copy.deepcopy(old)
    for row in derived["files"]:
        row["reports"] = [gap for gap in row["reports"]
                          if gap["message"] != "switch is not parsed in this subset"]
    derived["report_count"] = sum(len(row["reports"]) for row in derived["files"])
    derived["files_with_reports"] = sum(bool(row["reports"]) for row in derived["files"])
    require(canonical(derived) == canonical(prediction), "original switch prediction changed")
    expected = copy.deepcopy(prediction)
    if corpus == "commons":
        # Two existing trailing-dot literal parser limitations became visible in
        # the formerly opaque switch. Retain the failed 58-report prediction.
        rows = [row for row in expected["files"]
                if row["path"] == "org/apache/commons/lang3/JavaVersion.java"]
        require(len(rows) == 1 and not rows[0]["reports"], "JavaVersion prediction drift")
        rows[0]["reports"] = [
            {"classification": "parser_limit_or_invalid_syntax", "message": "expected name",
             "range": {"start_byte": at, "end_byte": at, "start_utf16": at, "end_utf16": at}}
            for at in (5071, 5075)]
        expected["report_count"] += 2
        expected["files_with_reports"] += 1
    require(canonical(actual) == canonical(expected), "unexplained switch parser difference")
    return old if stage == "baseline" else expected, read("baseline-source-sha256")

def check(corpus, actual, sources, stage="candidate"):
    expected, hashes = load(corpus, stage)
    require(canonical(actual) == canonical(expected), "complete switch parser records differ")
    require(hashes == {name: hashlib.sha256(data).hexdigest() for name, data in sources.items()},
            "complete switch source inventory differs")
    return expected
