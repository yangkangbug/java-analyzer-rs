#!/usr/bin/env python3
"""Offline Linux release-CLI measurements; no downloads, builds or Java execution.

The protocol JSON is written before warmup/measurements. Every measured command
starts a new supervisor and native worker; results are not warm-query timings.
Requires Python 3.11+. Inputs are the pinned Commons Lang sources and JMOD.
"""

import argparse
import contextlib
import copy
from datetime import datetime, timezone
import hashlib
import importlib.util
import io
import json
import math
import os
from pathlib import Path
import platform
import re
import shlex
import stat
import selectors
import signal
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
PREFIX = b"// Deterministic CLI measurement edit; original archive is unchanged.\n"
EDIT_FILE = "org/apache/commons/lang3/ArrayUtils.java"
WORKLOADS = ("inspect_details", "parser_report", "declaration_45", "source_edit_full_reload_45")


class MeasurementError(ValueError):
    pass


class CaseFailure(MeasurementError):
    def __init__(self, facts, cause):
        super().__init__(f"{facts['workload']}: {cause}")
        self.facts = facts


def require(condition, message):
    if not condition:
        raise MeasurementError(message)


def load_acceptance():
    # A fixed repository-owned module, never a project-selected Python plugin.
    spec = importlib.util.spec_from_file_location(
        "cli_acceptance", ROOT / "scripts/check-cli-acceptance.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def sha256(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def canonical_hash(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


_OUTPUT_IDENTITIES = {}


def write_json(path, value):
    """Publish complete JSON only; first creation is exclusive, updates are owned.

    A hard link publishes the first fully written temporary inode without an
    overwrite race. Later replacements target only this run's own report leaf.
    This is atomic visibility, not a filesystem durability guarantee.
    """
    path = Path(path).absolute()
    for part in (path, *path.parents):
        require(not part.is_symlink(), "report path contains a symlink")
    payload = (json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n").encode()
    path.parent.mkdir(parents=True, exist_ok=True)
    prior = _OUTPUT_IDENTITIES.get(path)
    if prior is not None:
        info = path.lstat()
        require(stat.S_ISREG(info.st_mode) and (info.st_dev, info.st_ino) == prior,
                "report was replaced outside this measurement run")
    descriptor, temporary = tempfile.mkstemp(prefix=".cli-measure-", dir=path.parent)
    temporary = Path(temporary)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(payload)
        if prior is None:
            os.link(temporary, path)  # Fails for any existing leaf, including symlinks.
        else:
            os.replace(temporary, path)
        info = path.lstat()
        _OUTPUT_IDENTITIES[path] = (info.st_dev, info.st_ino)
    finally:
        temporary.unlink(missing_ok=True)


def parse_proc_stat(text):
    # comm can contain spaces and parentheses; fields after the final ')' start at 3.
    fields = text[text.rindex(")") + 2:].split()
    return {"state": fields[0], "pgrp": int(fields[2]),
            "start_ticks": int(fields[19]), "rss_pages": int(fields[21])}


class ProcessGroupSampler:
    """Linux /proc observation, not unique physical memory or a guaranteed peak."""

    def __init__(self, pgid, interval):
        self.pgid = pgid
        self.interval = interval
        self.next_sample = 0.0
        self.last_sample = None
        self.max_gap_ms = 0.0
        self.samples = 0
        self.live_group_samples = 0
        self.failed_reads = 0
        self.peak_sum_rss_kib = None
        self.peak_process_count = 0
        self.processes = set()
        self.page_kib = os.sysconf("SC_PAGE_SIZE") / 1024
        self.available = True

    def sample(self, now):
        if now < self.next_sample:
            return
        if self.last_sample is not None:
            self.max_gap_ms = max(self.max_gap_ms, (now - self.last_sample) * 1000)
        self.last_sample = now
        self.next_sample = now + self.interval
        total = count = 0
        try:
            entries = list(Path("/proc").iterdir())
        except OSError:
            self.available = False
            self.failed_reads += 1
            return
        for entry in entries:
            if not entry.name.isdecimal():
                continue
            try:
                stat = parse_proc_stat((entry / "stat").read_text())
                if stat["pgrp"] != self.pgid:
                    continue
                self.processes.add((int(entry.name), stat["start_ticks"]))
                if stat["state"] != "Z":
                    count += 1
                    total += max(0, stat["rss_pages"]) * self.page_kib
            except (OSError, ValueError, IndexError):
                # A short-lived process may disappear between enumeration and read.
                self.failed_reads += 1
        self.samples += 1
        if count:
            self.live_group_samples += 1
            self.peak_sum_rss_kib = max(self.peak_sum_rss_kib or 0, total)
        self.peak_process_count = max(self.peak_process_count, count)

    def result(self):
        return {"available": self.available and self.live_group_samples > 0,
                "proc_enumeration_available": self.available, "samples": self.samples,
                "samples_with_live_group_processes": self.live_group_samples,
                "process_stat_read_failures_or_races": self.failed_reads,
                "peak_sum_rss_kib": self.peak_sum_rss_kib,
                "peak_simultaneously_observed_processes": self.peak_process_count,
                "distinct_processes_observed": len(self.processes),
                "max_observed_sample_gap_ms": self.max_gap_ms}


def measured_run(command, cwd, timeout=60.0, max_output=16 * 1024 * 1024,
                 sample_interval=0.005):
    """Time spawn through observed supervisor exit + stream EOF; return outputs and facts.

    waitid(WNOWAIT) observes exit without releasing the leader's PID/PGID.
    Group cleanup therefore cannot target a group that reused the leader's PID.
    wait4 is the sole reaper, after group cleanup. Linux child rusage includes
    reaped descendants: ru_maxrss is the maximum individual process high-water,
    not summed RSS. Cleanup/reaping and checks are outside successful latency.
    This runner is reused by offline unit tests with explicit Python fixtures.
    Production invocation argv contains only the supplied native CLI.
    """
    require(sys.platform == "linux" and hasattr(os, "wait4") and hasattr(os, "WNOWAIT"),
            "measurement requires Linux wait4/waitid(WNOWAIT) and /proc")
    require(timeout > 0 and math.isfinite(timeout), "timeout must be finite and positive")
    require(max_output > 0, "output limit must be positive")
    require(sample_interval > 0 and math.isfinite(sample_interval), "sampling interval must be finite and positive")
    output = {"stdout": bytearray(), "stderr": bytearray()}
    started = time.perf_counter()
    process = subprocess.Popen([str(p) for p in command], cwd=cwd,
                               env={"PATH": "/nonexistent"}, stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               start_new_session=True)
    sampler = ProcessGroupSampler(process.pid, sample_interval)
    deadline = started + timeout
    usage = None
    failure = None
    code = None
    cleanup_confirmed = False
    ended = None
    exit_observed = False

    def observe_exit():
        nonlocal exit_observed
        if not exit_observed:
            status = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
            exit_observed = status is not None

    def reap():
        nonlocal usage, code
        if code is not None:
            return
        pid, status, data = os.wait4(process.pid, os.WNOHANG)
        if pid:
            usage = data
            code = os.waitstatus_to_exitcode(status)
            # Prevent Popen.__del__/poll from attempting a second reap.
            process.returncode = code

    try:
        with selectors.DefaultSelector() as selector:
            for name in output:
                stream = getattr(process, name)
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, selectors.EVENT_READ, name)
            total = 0
            while True:
                now = time.perf_counter()
                sampler.sample(now)
                observe_exit()
                if exit_observed and not selector.get_map():
                    ended = time.perf_counter()
                    break
                if now >= deadline:
                    failure = "outer_timeout"
                    break
                wait = min(sample_interval, max(0, deadline - now))
                for key, _ in selector.select(wait):
                    try:
                        block = os.read(key.fileobj.fileno(), 65536)
                    except BlockingIOError:
                        continue
                    if not block:
                        selector.unregister(key.fileobj)
                        continue
                    total += len(block)
                    if total > max_output:
                        failure = "combined_output_limit"
                        break
                    output[key.data].extend(block)
                if failure:
                    break
    finally:
        ended = ended or time.perf_counter()
        # The leader has NOT been reaped, so its PID cannot have been reused.
        # Only the group we just created. This is harness containment, not a CLI
        # sandbox promise; descendants that deliberately leave it are not covered.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        cleanup_deadline = time.perf_counter() + 5
        while code is None and time.perf_counter() < cleanup_deadline:
            reap()
            if code is None:
                time.sleep(0.005)
        cleanup_confirmed = code is not None
        process.stdout.close()
        process.stderr.close()
    facts = {"wall_ms": (ended - started) * 1000, "exit_code": code,
             "failure": failure, "supervisor_reaped": cleanup_confirmed,
             "stdout_bytes": len(output["stdout"]), "stderr_bytes": len(output["stderr"]),
             "linux_wait4_maxrss_kib": usage.ru_maxrss if usage else None,
             "user_cpu_seconds_including_reaped_children": usage.ru_utime if usage else None,
             "system_cpu_seconds_including_reaped_children": usage.ru_stime if usage else None,
             "proc_sampling": sampler.result()}
    require(cleanup_confirmed, "supervisor cleanup was not confirmed within 5 seconds")
    return facts, bytes(output["stdout"]), bytes(output["stderr"])


def validate_envelope(facts, stdout, stderr, command, manifest):
    require(facts["failure"] is None, f"harness limit: {facts['failure']}")
    require(facts["exit_code"] == 0, f"CLI exit status {facts['exit_code']}")
    require(stderr == b"", "successful CLI wrote stderr")
    require(stdout.endswith(b"\n"), "CLI response missing final newline")
    response = json.loads(stdout)
    require(response["schema_version"] == 1, "wrong CLI response schema")
    require(response["command"] == command, "wrong CLI response command")
    require(response["snapshot"]["identity"] == "invocation_local", "wrong snapshot boundary")
    require(isinstance(response["snapshot"]["id"], str) and response["snapshot"]["id"], "missing snapshot ID")
    require(response["snapshot"]["manifest"] == str(manifest.resolve()), "wrong snapshot manifest")
    return response


def shifted_goldens(queries, targets, filename, prefix):
    prefix.decode("ascii")  # ASCII byte and UTF-16 offsets are identical here.
    shifted_queries, shifted_targets = copy.deepcopy(queries), copy.deepcopy(targets)
    for query in shifted_queries:
        if query[1] == filename:
            query[2] = str(int(query[2]) + len(prefix))
    for target in shifted_targets:
        if target[1] == filename:
            target[5] = str(int(target[5]) + len(prefix))
            target[6] = str(int(target[6]) + len(prefix))
    return shifted_queries, shifted_targets


def prepare_commons(stage, source, platform_file, native, worker_timeout_ms):
    a = load_acceptance()
    specs = json.loads((ROOT / "scripts/acceptance-inputs.json").read_text())
    security = a.input_helpers()
    security.reject_symlink_path(source)
    security.reject_symlink_path(platform_file)
    source, platform_file = source.resolve(strict=True), platform_file.resolve(strict=True)
    require(platform_file.is_file(), "platform must be a regular file")
    require(platform_file.stat().st_size <= specs["jdk"]["max_file"], "platform size budget exceeded")
    require(sha256(platform_file) == specs["jdk"]["jmod_sha256"], "platform SHA-256 mismatch")
    source_root, notices, cwd = stage / "sources", stage / "notices", stage / "unrelated-cwd"
    for directory in (source_root, notices, cwd):
        directory.mkdir()
    sources = a.extract_sources(source, specs["sources"], source_root, notices)
    queries, targets = a.load_goldens(sources)
    manifest = stage / "config/project.json"
    requests = stage / "queries/requests.json"
    config = {"schema_version": 1, "compilations": [{
        "id": a.COMPILATION, "source_roots": ["../sources"], "dependencies": [],
        "classpath": [], "target_release": 21,
        "platform": [{"kind": "jmod", "path": os.path.relpath(platform_file, manifest.parent)}]}]}
    a.write_json(manifest, config)
    a.write_json(requests, a.requests_for(queries))
    edited_queries, edited_targets = shifted_goldens(queries, targets, EDIT_FILE, PREFIX)
    edited_requests = stage / "queries/edited.json"
    a.write_json(edited_requests, a.requests_for(edited_queries))
    edited_sources = dict(sources)
    edited_sources[EDIT_FILE] = PREFIX + sources[EDIT_FILE]
    inspection = {}

    def check_inspection(response):
        a.check_inspection(response, source_root, sources, a.PLATFORM_CLASSES, platform_file)
        inspection.update(response)

    def check_parser(response):
        require(bool(inspection), "inspection must run before parser report")
        a.check_parser_report(response, inspection, source_root, sources, print_inventory=False)

    def check_declarations(response):
        a.check_declarations(response, queries, targets, source_root, platform_file, sources, effective_binary=True)

    def check_edit(response):
        a.check_declarations(response, edited_queries, edited_targets, source_root, platform_file, edited_sources, effective_binary=True)

    cases = []
    for name, command, request, check in (
            (WORKLOADS[0], "inspect", None, check_inspection),
            (WORKLOADS[1], "parser-report", None, check_parser),
            (WORKLOADS[2], "declaration", requests, check_declarations),
            (WORKLOADS[3], "declaration", edited_requests, check_edit)):
        argv = [native, command, "--manifest", manifest, "--timeout-ms", str(worker_timeout_ms)]
        if command == "inspect":
            argv.append("--details")
        if request:
            argv.extend(["--requests", request])
        case = {"name": name, "command": command, "argv": argv, "manifest": manifest,
                "cwd": cwd, "check": check}
        if name == WORKLOADS[3]:
            case["before"] = lambda: (source_root / EDIT_FILE).write_bytes(edited_sources[EDIT_FILE])
            case["after"] = lambda: (source_root / EDIT_FILE).write_bytes(sources[EDIT_FILE])
        cases.append(case)
    normalized_config = copy.deepcopy(config)
    normalized_config["compilations"][0]["platform"][0]["path"] = "<pinned-java.base.jmod>"
    metadata = {"name": "Apache Commons Lang 3.17.0", "source_count": len(sources),
                "source_bytes": sum(map(len, sources.values())), "platform_classes": a.PLATFORM_CLASSES,
                "source_artifact_sha256": sha256(source), "source_artifact_bytes": source.stat().st_size,
                "platform_sha256": sha256(platform_file), "platform_bytes": platform_file.stat().st_size,
                "input_provenance": specs, "query_count": len(queries), "candidate_count": len(targets),
                "source_inventory_sha256": canonical_hash({name: hashlib.sha256(data).hexdigest()
                                                            for name, data in sorted(sources.items())}),
                "normalized_manifest": normalized_config,
                "normalized_manifest_sha256": canonical_hash(normalized_config),
                "requests_sha256": sha256(requests), "edited_requests_sha256": sha256(edited_requests),
                "edit": {"file": EDIT_FILE, "prefix_utf8": PREFIX.decode(),
                         "original_sha256": hashlib.sha256(sources[EDIT_FILE]).hexdigest(),
                         "edited_sha256": hashlib.sha256(edited_sources[EDIT_FILE]).hexdigest()}}
    return cases, metadata


def prepare_second(stage, inputs_dir, native, worker_timeout_ms):
    # This is a fixed, reviewed repository checker, not a user-supplied plugin.
    spec = importlib.util.spec_from_file_location("second_project_cli_acceptance",
                                                ROOT / "scripts/check-second-project.py")
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    fixture = helper.prepare_fixture(inputs_dir, stage)
    cwd = stage / "unrelated-cwd"
    cwd.mkdir()
    cases = []
    for name, command in (("gson_inspect_summary", "inspect"),
                          ("gson_parser_report", "parser-report"),
                          ("gson_declaration_16", "declaration")):
        argv = [native, command, "--manifest", fixture.manifest, "--timeout-ms", str(worker_timeout_ms)]
        if command == "declaration":
            argv.extend(["--requests", fixture.requests])
        cases.append({"name": name, "command": command, "argv": argv,
                      "manifest": fixture.manifest, "cwd": cwd,
                      "check": lambda response, command=command: helper.check_response(command, response, fixture)})
    metadata = copy.deepcopy(fixture.metadata)
    metadata["fixture_sha256"] = {path: sha256(ROOT / path) for path in (
        "scripts/check-second-project.py", "scripts/fetch-second-project-inputs.py",
        "scripts/second-project-inputs.json", "examples/second_project/query-plan.json",
        "examples/second_project/expected.json", "examples/second_project/parser-errors.tsv")}
    metadata["requests_sha256"] = sha256(fixture.requests)
    return cases, metadata


def setup_command(argv, max_output=65536):
    result = subprocess.run(argv, cwd=ROOT, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, timeout=15, check=True)
    require(len(result.stdout) < max_output, "setup metadata output too large")
    return result.stdout.decode().strip()


def source_identity():
    files = sorted(list((ROOT / "src").rglob("*.rs")) +
                   [ROOT / name for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml")])
    inventory = {path.relative_to(ROOT).as_posix(): sha256(path) for path in files}
    return {"git_revision": setup_command(["git", "rev-parse", "HEAD"]),
            "checkout_dirty_at_measurement": bool(setup_command(["git", "status", "--porcelain"])),
            "git_tracked_diff_sha256": hashlib.sha256(setup_command(["git", "diff", "--no-ext-diff", "--no-textconv", "HEAD", "--"], 4 * 1024 * 1024).encode()).hexdigest(),
            "rust_build_input_files": inventory, "rust_build_inputs_sha256": canonical_hash(inventory),
            "toolchain_evidence": "compiler/target recorded in build receipt; no compiler or Cargo invoked by measurement harness",
            "fixture_sha256": {name: sha256(ROOT / name) for name in (
                "scripts/check-cli-acceptance.py", "scripts/fetch-acceptance-inputs.py",
                "scripts/acceptance-inputs.json", "examples/real_source/queries.tsv",
                "examples/real_source/targets.tsv", "examples/real_source/parser-errors.tsv")}}


def portable_build_provenance(build, identity, binary_hash, receipt_hash):
    """Copy constrained facts, never arbitrary receipt command/compiler strings."""
    for key in ("git_revision", "rust_build_inputs_sha256"):
        require(build[key] == identity[key], f"build provenance {key} differs from checkout")
    require(build["binary_sha256"] == binary_hash, "build provenance binary checksum mismatch")
    argv = shlex.split(build["build_command"])
    require(argv[:2] == ["cargo", "build"] and "--locked" in argv and "--release" in argv,
            "provenance must declare a locked release build")
    for flag, value in (("--features", "cli"), ("--bin", "java-analyzer")):
        require(flag in argv and argv.index(flag) + 1 < len(argv) and argv[argv.index(flag) + 1] == value,
                f"build provenance requires {flag} {value}")
    compiler = {}
    constraints = {"release": r"1\.96\.0", "commit-hash": r"[a-f0-9]{40}",
                   "commit-date": r"[0-9]{4}-[0-9]{2}-[0-9]{2}",
                   "host": r"x86_64-unknown-linux-gnu", "LLVM version": r"[0-9]+(?:\.[0-9]+){1,2}"}
    for line in build["rustc_version"].splitlines():
        if ": " in line:
            key, value = line.split(": ", 1)
            if key in constraints:
                require(re.fullmatch(constraints[key], value) is not None, "invalid compiler provenance field")
                compiler[key] = value
    require(set(compiler) == set(constraints), "incomplete compiler provenance")
    result = {"binary_sha256": binary_hash, "git_revision": build["git_revision"],
              "rust_build_inputs_sha256": build["rust_build_inputs_sha256"],
              "receipt_sha256": receipt_hash,
              "original_build_command_sha256": hashlib.sha256(build["build_command"].encode()).hexdigest(),
              "build_command_normalized": "cargo build --locked --release --features cli --bin java-analyzer (output location omitted)",
              "rustc": compiler}
    for key, pattern in (("git_tree", r"[a-f0-9]{40}"), ("version", r"[0-9]+\.[0-9]+\.[0-9]+"),
                         ("target", r"x86_64-unknown-linux-gnu")):
        if key in build:
            require(isinstance(build[key], str) and re.fullmatch(pattern, build[key]), f"invalid build {key}")
            result[key] = build[key]
    require(isinstance(build.get("source_dirty"), bool), "receipt must state source_dirty")
    result["source_dirty"] = build["source_dirty"]
    if "target" in result:
        result["build_command_normalized"] += " --target " + result["target"]
    if "--offline" in argv:
        result["build_command_normalized"] += " --offline"
    return result


def host_identity():
    facts = {"system": platform.system(), "kernel_release": platform.release(),
             "architecture": platform.machine(), "python": platform.python_version(),
             "logical_cpus": os.cpu_count(), "page_bytes": os.sysconf("SC_PAGE_SIZE")}
    try:
        facts["cpu_affinity_count"] = len(os.sched_getaffinity(0))
    except (AttributeError, OSError):
        facts["cpu_affinity_count"] = None
    try:
        facts["os_release"] = {key: value for key, value in platform.freedesktop_os_release().items()
                               if key in ("ID", "VERSION_ID", "PRETTY_NAME")}
    except OSError:
        facts["os_release"] = None
    try:
        cpu = Path("/proc/cpuinfo").read_text()
        facts["cpu_model"] = next(line.split(":", 1)[1].strip() for line in cpu.splitlines()
                                  if line.startswith("model name"))
        memory = Path("/proc/meminfo").read_text()
        facts["meminfo_kib"] = {line.split(":")[0]: int(line.split()[1])
                                for line in memory.splitlines()
                                if line.split(":")[0] in ("MemTotal", "MemAvailable", "SwapTotal")}
    except (OSError, StopIteration, ValueError):
        facts["proc_host_details_unavailable"] = True
    # Fixed public quota filenames only; do not expose cgroup identities or paths.
    facts["cgroup_v2_limits"] = {}
    for name in ("memory.max", "memory.high", "cpu.max"):
        try:
            facts["cgroup_v2_limits"][name] = (Path("/sys/fs/cgroup") / name).read_text().strip()
        except OSError:
            facts["cgroup_v2_limits"][name] = None
    return facts


def protocol(repetitions, timeout, max_output, interval, worker_timeout_ms):
    return {"schema_version": 1, "repetitions": repetitions, "warmups_per_workload": 1,
            "order": "all warmups in workload order, then serial round-robin measured rounds",
            "workloads": list(WORKLOADS), "outer_timeout_seconds": timeout,
            "worker_timeout_ms": worker_timeout_ms, "combined_output_limit_bytes": max_output,
            "cleanup_grace_seconds": 5, "proc_sample_interval_ms": interval * 1000,
            "environment": {"PATH": "/nonexistent"},
            "timing_boundary": "perf_counter before Popen to supervisor exit observed with waitid(WNOWAIT) and both output streams EOF; monitor overhead included; JSON checks, group cleanup and final wait4 reap excluded",
            "edit_timing_boundary": "before private source write to complete CLI result; restore excluded; no fsync or durability claim",
            "cache_condition": "OS-cache-warm intention after setup/hash reads and one warmup; residency not controlled or guaranteed; fresh process every command",
            "not_measured": ["true cold-cache I/O", "isolated load/preparation phases", "warm prepared-query latency", "incremental reload", "preserved snapshot rollback", "unique physical memory"],
            "correctness_gate": "Every warmup and measured response passes pinned source inventory/parser-label/range checks or exact ordered declaration goldens; one index per batch; no time/RSS performance thresholds",
            "statistics": "raw samples and minimum/median/maximum; no p95 estimate with 5-7 observations",
            "memory_semantics": {"linux_wait4_maxrss_kib": "Linux wait4 rusage of supervisor including descendants it reaped; maximum individual-process high-water, not simultaneous aggregate or sum of peaks",
                                 "proc_peak_sum_rss_kib": "largest sampled sum of RSS for live processes in created process group; supervisor and worker included when observed; shared pages double counted, reads non-atomic, short-lived peaks/processes may be missed, /proc RSS itself can be approximate; polling overhead included",
                                 "unavailable_or_races": "Reported explicitly, never replaced with zero as evidence of no memory use; process enumeration read races can include unrelated processes"},
            "safety": ["No downloads or builds in harness", "JMOD read only as data", "No Java/JVM/JNI/JDT LS or project scripts invoked", "Harness kills its new process group on timeout/output failure; not a security sandbox or protection from descendants leaving that group", "No OS memory quota or cache/security settings changed", "Host scheduling, container quotas and concurrent workloads affect results"]}


def measure_case(case, options, measured, repetition):
    edit_started = time.perf_counter() if "before" in case else None
    try:
        if "before" in case:
            case["before"]()
        edit_write_ms = (time.perf_counter() - edit_started) * 1000 if edit_started is not None else None
        facts, stdout, stderr = measured_run(case["argv"], case["cwd"], **options)
        if edit_write_ms is not None:
            facts["edit_to_result_ms"] = edit_write_ms + facts["wall_ms"]
        facts.update(workload=case["name"], measured=measured, repetition=repetition)
        try:
            response = validate_envelope(facts, stdout, stderr, case["command"], case["manifest"])
            with contextlib.redirect_stdout(io.StringIO()):
                case["check"](response)
        except (OSError, ValueError, KeyError, TypeError, IndexError, RuntimeError) as error:
            facts["correctness_passed"] = False
            facts["failure_category"] = type(error).__name__
            raise CaseFailure(facts, error) from error
        facts["correctness_passed"] = True
        return facts
    finally:
        if "after" in case:
            case["after"]()


def summarize(samples):
    result = {}
    for name in dict.fromkeys(sample["workload"] for sample in samples):
        rows = [sample for sample in samples if sample["workload"] == name and sample["measured"]]
        summary = {"n": len(rows)}
        for key in ("wall_ms", "linux_wait4_maxrss_kib", "edit_to_result_ms"):
            values = [row[key] for row in rows if row.get(key) is not None]
            if values:
                summary[key] = {"min": min(values), "median": statistics.median(values), "max": max(values)}
        values = [row["proc_sampling"]["peak_sum_rss_kib"] for row in rows
                  if row["proc_sampling"]["available"] and row["proc_sampling"]["peak_sum_rss_kib"] is not None]
        if values:
            summary["sampled_group_sum_rss_kib"] = {"min": min(values), "median": statistics.median(values), "max": max(values)}
        result[name] = summary
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sources_jar", type=Path)
    parser.add_argument("java_base_jmod", type=Path)
    parser.add_argument("--native", type=Path, default=ROOT / "target/release/java-analyzer")
    parser.add_argument("--output", type=Path, required=True, help="new JSON result file; .protocol.json is written first")
    parser.add_argument("--second-inputs-dir", type=Path,
                        help="optional verified local Gson fixture inputs; adds three fixed workloads")
    parser.add_argument("--build-provenance", type=Path, required=True,
                        help="operator-authored JSON: binary_sha256, git_revision, rust_build_inputs_sha256, build_command, rustc_version; checksum/digests are checked, historical build is not independently attested")
    parser.add_argument("--repetitions", type=int, choices=(5, 6, 7), default=7)
    parser.add_argument("--timeout-seconds", type=float, default=60)
    parser.add_argument("--max-output-bytes", type=int, default=16 * 1024 * 1024)
    parser.add_argument("--sample-interval-ms", type=float, default=5)
    args = parser.parse_args()
    result = None
    try:
        require(sys.platform == "linux", "this measurement protocol currently supports Linux only")
        require(math.isfinite(args.timeout_seconds) and 30 < args.timeout_seconds <= 600,
                "outer timeout must exceed CLI's 30s work budget and be at most 600s")
        require(1024 <= args.max_output_bytes <= 64 * 1024 * 1024, "output limit must be 1 KiB through 64 MiB")
        require(math.isfinite(args.sample_interval_ms) and 1 <= args.sample_interval_ms <= 100,
                "sampling interval must be 1..100 milliseconds")
        protocol_path = args.output.with_suffix(".protocol.json")
        require(not args.output.exists() and not protocol_path.exists(), "result/protocol already exists; choose a new output path")
        native = args.native.resolve(strict=True)
        require(native.is_file() and os.access(native, os.X_OK), "native must be an executable file")
        identity = source_identity()
        build = portable_build_provenance(json.loads(args.build_provenance.read_text()), identity,
                                          sha256(native), sha256(args.build_provenance))
        interval = args.sample_interval_ms / 1000
        plan = protocol(args.repetitions, args.timeout_seconds, args.max_output_bytes, interval, 30000)
        with tempfile.TemporaryDirectory(prefix="java-analyzer-measure-") as temporary:
            cases, inputs = prepare_commons(Path(temporary), args.sources_jar.absolute(),
                                            args.java_base_jmod.absolute(), native, 30000)
            if args.second_inputs_dir is not None:
                second_cases, second_inputs = prepare_second(Path(temporary) / "second-project",
                                                            args.second_inputs_dir, native, 30000)
                cases.extend(second_cases)
                inputs = {"commons_lang": inputs, "gson": second_inputs}
            plan["workloads"] = [case["name"] for case in cases]
            result = {"schema_version": 1, "status": "running", "protocol": plan,
                      "started_utc": datetime.now(timezone.utc).isoformat(),
                      "host": host_identity(), "source": identity, "inputs": inputs,
                      "build_provenance": build,
                      "build_provenance_trust": "operator declaration, cross-checked against binary and present Rust-input digests; not independent historical build attestation",
                      "harness_sha256": sha256(Path(__file__)), "samples": []}
            write_json(protocol_path, {key: value for key, value in result.items() if key not in ("status", "samples")})
            print("Protocol written before warmups; measuring fresh CLI processes.", flush=True)
            options = dict(timeout=args.timeout_seconds, max_output=args.max_output_bytes, sample_interval=interval)
            for repetition in range(args.repetitions + 1):
                for case in cases:
                    sample = measure_case(case, options, repetition != 0, repetition)
                    result["samples"].append(sample)
                    write_json(args.output, result)
                    print(f"{'warmup' if repetition == 0 else 'measured'} {case['name']} round={repetition} wall_ms={sample['wall_ms']:.2f}", flush=True)
            require(sha256(native) == build["binary_sha256"], "binary changed during measurements")
            require(sha256(Path(__file__)) == result["harness_sha256"], "harness changed during measurements")
            fixture_hashes = dict(identity["fixture_sha256"])
            if args.second_inputs_dir is not None:
                fixture_hashes.update(second_inputs["fixture_sha256"])
            require(all(sha256(ROOT / name) == checksum for name, checksum in fixture_hashes.items()),
                    "fixture/checker changed during measurements")
            require(setup_command(["git", "rev-parse", "HEAD"]) == identity["git_revision"],
                    "source revision changed during measurements")
            result["post_run_identity_checks_passed"] = True
            result["summary"] = summarize(result["samples"])
            result["status"] = "passed"
            result["finished_utc"] = datetime.now(timezone.utc).isoformat()
            write_json(args.output, result)
            print("PASS correctness; performance observations only, no speed/RSS threshold.")
    except (OSError, ValueError, KeyError, TypeError, IndexError, RuntimeError, subprocess.SubprocessError) as error:
        if result is not None:
            if isinstance(error, CaseFailure):
                result["samples"].append(error.facts)
            result["status"] = "failed"
            result["finished_utc"] = datetime.now(timezone.utc).isoformat()
            # Exception text can contain private staging/input paths. Preserve the
            # category, not absolute paths or source contents, in portable evidence.
            result["failure_category"] = type(error).__name__
            write_json(args.output, result)
        parser.exit(1, f"MEASUREMENT FAILED ({type(error).__name__}); see local exception details: {error}\n")


if __name__ == "__main__":
    main()
