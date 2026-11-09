# Native CLI execution boundary

The public `java-analyzer` commands run in a **separate native worker process**
by default. The supervisor starts only its own executable, using its resolved
executable path rather than PATH. There is no arbitrary executable option, Java,
JNI, JDT LS, build-tool invocation, shell, network client, or persistent server.
The reusable library APIs are unchanged and still run inside their caller.

This is a one-shot reliability boundary for trusted local projects, **not a
security sandbox or a whole-process resource guarantee**. Linux is the currently
tested platform. The implementation uses portable Rust standard-library process,
pipe, thread and monotonic-clock facilities, without new dependencies or unsafe
code. macOS and Windows behavior is not release-certified by Linux tests.

## Calling it

All commands accept these two optional flags, including `--help` and `--version`:

```sh
java-analyzer declaration --manifest project.json --requests requests.json \
  --timeout-ms 10000
java-analyzer parser-report --manifest project.json --cancel-on-stdin-close
```

- `--timeout-ms N`: wall-clock work budget, default **30000 ms**, minimum **1 ms**,
  maximum **300000 ms**. Only ASCII decimal digits are accepted. Zero, signs,
  whitespace, fractions, overflow, missing values and repeated flags fail with
  `usage`. There is no unlimited or disable-timeout option. Very small budgets
  can expire during native process startup, even for `--help`.
- `--cancel-on-stdin-close`: opt-in cancellation. The caller creates and keeps open
  a pipe to the supervisor's stdin; closing every writer requests cancellation.
  Bytes received on stdin are discarded, not interpreted as a protocol. An
  already-closed stdin cancels immediately. Without this flag stdin is ignored,
  so ordinary noninteractive invocations retain their previous behavior.

Keep reading **both stdout and stderr concurrently** and wait for the supervisor's
exit status. Do not synchronously run this CLI on an editor UI thread. On success,
exit 0 and the existing schema-1 response are unchanged. The supervisor forwards
original worker response bytes, including snapshot identity, ranges, provenance,
query ordering, refusal statuses and parser classifications. Each invocation
still loads one snapshot and each declaration batch prepares one index.

A timeout or cancellation discards the whole worker response. It does not return
a successful query prefix, expose a partial snapshot, or make an old snapshot
current. Cancellation racing completion returns whichever terminal event the
supervisor observes first; it cannot retract an already completed response.

## What is bounded

The monotonic deadline starts immediately before spawning the worker. Manifest
and request reads, project loading, parsing, index preparation, query evaluation,
response serialization and worker output capture run under that deadline. A
supervisor polling loop checks approximately every **5 ms**, subject to OS
scheduling. This is elapsed time, not a CPU-time quota.

The supervisor drains stdout and stderr concurrently. It accepts at most **16 MiB
stdout** and **1 MiB stderr**, checking before extending each byte buffer. This is
at most **17 MiB of captured stream content**, not a limit on allocated memory.
The worker still enforces its existing 16 MiB serialized-response and 10000-item
limits. Exceeding a supervisor stream limit is an error, never truncation. Each
reader reports once through a capacity-one channel; output cannot build up in an
unbounded message queue.

If a worker exits normally but a descendant or another holder keeps a pipe open,
the supervisor allows at most **100 ms** to finish draining, within the original
deadline. It then returns `worker_output` (or `execution_timeout` if the deadline
expires first). It never joins a blocked output reader or waits forever for EOF.
The actual analyzer worker creates no descendants. Reader threads with inherited
pipes are abandoned on this error and terminate when the supervisor process exits;
this design is for the one-shot binary, not a reusable in-process supervisor API.

On timeout, cancellation, output failure or other supervision error, the
supervisor closes its worker-stdin lease, requests termination of its direct
worker, and polls for reaping for at most **250 ms**. That cleanup grace is
additional to the requested work budget. `error.worker_reaped` is `true` only
when the supervisor has observed and reaped the child's exit. `false` means
cleanup was not confirmed within the grace; the caller must not assume the worker
has stopped. Errors before a worker was started omit this field.

The worker starts a stdin lease watcher before reading any project data. The
supervisor keeps the sole writer open. If the supervisor dies, including SIGKILL
on Linux, EOF makes the worker exit. There is no custom signal handler: killing
the supervisor does **not** produce a JSON cancellation response. A dead
supervisor cannot reap its child; an orphan's eventual reaping belongs to the OS
or container's reaper. Tests distinguish a terminated zombie from a running
worker. Holding a copy of an input pipe's writer open delays EOF cancellation.

## What is not bounded or promised

- No OS CPU quota, memory/RSS/address-space limit, filesystem sandbox, reduced
  privilege, syscall restriction, process-group kill or arbitrary descendant-tree
  cleanup is installed. No system or security settings are changed.
- Recursive parsing can still overflow the **worker's** stack. Panic, abort and
  signal exits are reported by the supervisor as `worker_failed`; this does not
  repair or expand supported Java syntax. Worker OOM may be reported similarly,
  but an OS-wide OOM event can also kill the supervisor.
- Native process spawn, OS calls and scheduling are not hard-real-time or
  preemptible. Kernel uninterruptible I/O, a suspended supervisor or machine
  resource exhaustion can delay polling, termination and cleanup.
- The supervisor's own argument parsing, executable lookup, thread startup,
  bounded-envelope JSON validation and final output writes are not interrupted
  by the worker deadline. In particular, a caller that stops reading can block
  final stdout/stderr delivery indefinitely. Use caller-side asynchronous reads
  and an outer watchdog when an end-to-end deadline is required.
- Capturing a bounded JSON response and validating its envelope can allocate
  additional memory. Input byte/archive limits, tree allocations and indexes
  remain the core loader's existing limits, not total memory limits.
- Disk input loading is not atomic. There is no unsaved-buffer overlay, automatic
  reload transaction, persistent snapshot service, or editor cancellation protocol.
- The internal `__java_analyzer_worker_v1` entry point and lease transport are
  private implementation details, not supported public commands or a security
  barrier. Invoke documented commands only. No environment variable selects an
  alternate worker, executable, fault mode or timeout bypass.

## Machine-readable failures

All recoverable supervisor errors exit **2**, leave stdout empty, and write one
schema-1 JSON object to stderr, for example:

```json
{
  "schema_version": 1,
  "error": {
    "code": "execution_timeout",
    "message": "native worker exceeded 10000 ms wall-clock deadline",
    "worker_reaped": true
  }
}
```

The new stable codes are:

| Code | Meaning |
| --- | --- |
| `execution_timeout` | Work/capture did not finish before the wall-clock deadline. |
| `execution_cancelled` | Opt-in stdin cancellation was observed. |
| `execution_io` | Executable lookup, spawn, thread/pipe setup, polling or pipe reading failed. |
| `worker_failed` | The worker exited by signal/abort or with an unexpected exit code. |
| `worker_output_limit` | Captured worker stdout or stderr exceeded its fixed byte cap. |
| `worker_output` | Missing, malformed, contradictory, wrong-version/command output, or pipes left open after worker exit. |

Worker exit 0 requires one successful JSON envelope on stdout and empty stderr.
Worker exit 2 requires one known error envelope on stderr and empty stdout. Both
require a final newline. Top-level protocol fields are typed and reject duplicate
or unknown fields; source/result objects retain their existing contract. Multiple
JSON objects, a wrong command/snapshot shape, panic text, an error on a success
exit, and a response mixed with stderr noise are rejected instead of forwarded.
Original ordinary CLI errors keep their existing codes, paths and query indexes.
Human-readable messages and OS exit-status descriptions are not stable fields.

The supervisor itself cannot emit a JSON error after it is killed or aborts.
Final transport failure can leave an OS-written stdout prefix before `output_io`;
output backpressure/failure is separate from the all-or-nothing validation rule.

## Evidence and remaining gates

`cargo test --locked --features cli --bin java-analyzer --test cli` covers:

- Previous declaration, parser-report, configuration and byte/range contracts.
- Real-binary timeout, EOF cancellation and deeply recursive parser stack abort.
- Test-only deterministic hang, cancellation, abort and abnormal exit fixtures.
- Stream byte boundaries, simultaneous output draining and inherited output pipes.
- Missing/multiple/duplicate/malformed/contradictory output and exit statuses.
- Lease EOF cleanup and, on Linux, actual supervisor SIGKILL with worker termination.

Fault fixtures are compiled only into the Rust test harness, not shipped in the
product executable. Tests invoke only their own native executable. The fixed
real-project [CLI acceptance workload](../examples/cli_acceptance/README.md)
remains a separate compatibility gate. These tests are not a claim of adversarial
security, end-to-end real-time deadlines, memory containment, cross-platform
certification, or completion of every [preview gate](PREVIEW_PLAN.md).
