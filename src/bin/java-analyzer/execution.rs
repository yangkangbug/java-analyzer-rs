//! Native one-shot process supervision. This is not a sandbox or an OS resource quota.
//! Reader threads are deliberately never joined: an inherited pipe must not hold
//! the supervisor alive after its direct worker has exited. See CLI_EXECUTION.md.
use super::{Error, MAX_RESPONSE_BYTES};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    io::{self, Read},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

pub(super) const WORKER_ARGUMENT: &str = "__java_analyzer_worker_v1";
const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const MAX_TIMEOUT_MS: u64 = 300_000;
const MAX_ERROR_BYTES: usize = 1024 * 1024;
const POLL: Duration = Duration::from_millis(5);
const CLEANUP_GRACE: Duration = Duration::from_millis(250);
const PIPE_GRACE: Duration = Duration::from_millis(100);

pub(super) fn capabilities() -> Value {
    json!({
        "mode":"native_self_process",
        "default_timeout_ms":DEFAULT_TIMEOUT_MS,
        "max_timeout_ms":MAX_TIMEOUT_MS,
        "cleanup_grace_ms":CLEANUP_GRACE.as_millis(),
        "pipe_grace_ms":PIPE_GRACE.as_millis(),
        "worker_stdout_bytes":MAX_RESPONSE_BYTES,
        "worker_stderr_bytes":MAX_ERROR_BYTES,
        "cancellation":"optional_stdin_eof",
        "parent_lifetime":"worker_stdin_lease",
        "kill_scope":"direct_worker_only",
        "cpu_quota":false,"memory_limit":false,"security_sandbox":false,
        "final_output_backpressure_bounded":false
    })
}

struct Options {
    args: Vec<String>,
    timeout: Duration,
    cancel_on_stdin_close: bool,
}
impl Options {
    fn parse(args: Vec<String>) -> Result<Self, Error> {
        let mut forwarded = Vec::new();
        let mut timeout = None;
        let mut cancel_on_stdin_close = false;
        let mut iter = args.into_iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--timeout-ms" => {
                    if timeout.is_some() {
                        return Err(Error::new("usage", "duplicate option --timeout-ms"));
                    }
                    let value = iter.next().ok_or_else(|| Error::new("usage", "missing value for --timeout-ms"))?;
                    let ms = value
                        .parse::<u64>()
                        .ok()
                        .filter(|ms| (1..=MAX_TIMEOUT_MS).contains(ms))
                        .filter(|_| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
                        .ok_or_else(|| Error::new("usage", "--timeout-ms must be decimal digits in 1..300000"))?;
                    timeout = Some(Duration::from_millis(ms));
                }
                "--cancel-on-stdin-close" => {
                    if cancel_on_stdin_close {
                        return Err(Error::new("usage", "duplicate option --cancel-on-stdin-close"));
                    }
                    cancel_on_stdin_close = true;
                }
                // Preserve option values verbatim, even when they look like a supervision option.
                "--manifest" | "--requests" | "--project" | "--source-set" | "--file" | "--byte" | "--utf16"
                | "--overlays" | "--line" | "--character" | "--job" => {
                    forwarded.push(arg);
                    if let Some(value) = iter.next() {
                        forwarded.push(value);
                    }
                }
                _ => forwarded.push(arg),
            }
        }
        if forwarded.first().is_some_and(|arg| arg == WORKER_ARGUMENT) {
            return Err(Error::new("usage", "reserved internal worker argument"));
        }
        Ok(Self {
            args: forwarded,
            timeout: timeout.unwrap_or(Duration::from_millis(DEFAULT_TIMEOUT_MS)),
            cancel_on_stdin_close,
        })
    }
}

/// A closed lease terminates the worker, including when the supervisor is killed.
/// No analysis code reads stdin or inherits the lease in another process.
pub(super) fn watch_parent() -> Result<(), Error> {
    thread::Builder::new()
        .name("java-analyzer-parent-lease".into())
        .spawn(|| {
            let mut stdin = io::stdin().lock();
            let mut buf = [0; 64];
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) => std::process::exit(2),
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => std::process::exit(2),
                }
            }
        })
        .map(|_| ())
        .map_err(|error| Error::new("execution_io", format!("cannot watch parent lease: {error}")))
}

pub(super) struct Response {
    pub success: bool,
    pub bytes: Vec<u8>,
}

pub(super) fn run(args: Vec<String>) -> Result<Response, Error> {
    let options = Options::parse(args)?;
    let cancellation = if options.cancel_on_stdin_close {
        let (tx, rx) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("java-analyzer-cancellation".into())
            .spawn(move || {
                let mut stdin = io::stdin().lock();
                let mut buf = [0; 256];
                let outcome = loop {
                    match stdin.read(&mut buf) {
                        Ok(0) => break Ok(()),
                        Ok(_) => {}
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(error) => break Err(error),
                    }
                };
                let _ = tx.send(outcome);
            })
            .map_err(|error| Error::new("execution_io", format!("cannot watch cancellation: {error}")))?;
        Some(rx)
    } else {
        None
    };
    let executable = std::env::current_exe()
        .map_err(|error| Error::new("execution_io", format!("cannot locate native executable: {error}")))?;
    let mut command = Command::new(executable);
    command.arg(WORKER_ARGUMENT).args(&options.args);
    let captured = supervise(&mut command, options.timeout, cancellation.as_ref())?;
    validate(captured, &options.args)
}

struct Worker {
    child: Child,
    lease: Option<ChildStdin>,
    reaped: bool,
}
impl Worker {
    /// Never call wait or join here: kernel waits and inherited pipes are not bounded.
    fn stop(&mut self) -> bool {
        self.lease.take();
        if self.reaped {
            return true;
        }
        let _ = self.child.kill();
        let deadline = Instant::now() + CLEANUP_GRACE;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    self.reaped = true;
                    return true;
                }
                Ok(None) if Instant::now() < deadline => thread::sleep(POLL),
                _ => return false,
            }
        }
    }
    fn error(&mut self, mut error: Error) -> Error {
        error.worker_reaped = Some(self.stop());
        error
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if !self.reaped {
            // Last-resort cleanup on unwinding. Normal errors already ran bounded stop.
            self.lease.take();
            let _ = self.child.kill();
            let _ = self.child.try_wait();
        }
    }
}

struct Captured {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}
type Cancellation = Receiver<io::Result<()>>;
type PipeResult = Result<Vec<u8>, Error>;
fn reader(reader: impl Read + Send + 'static, limit: usize) -> io::Result<Receiver<PipeResult>> {
    let (tx, rx) = mpsc::sync_channel(1);
    thread::Builder::new().name("java-analyzer-output".into()).spawn(move || {
        let _ = tx.send(read_bounded(reader, limit));
    })?;
    Ok(rx)
}
fn read_bounded(mut reader: impl Read, limit: usize) -> PipeResult {
    let mut bytes = Vec::new();
    let mut buf = [0; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => return Ok(bytes),
            Ok(n) => {
                if n > limit - bytes.len() {
                    return Err(Error::new(
                        "worker_output_limit",
                        "native worker exceeded a protocol stream byte limit",
                    ));
                }
                bytes.extend_from_slice(&buf[..n]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(Error::new("execution_io", format!("cannot read native worker output: {error}"))),
        }
    }
}
fn receive(rx: &Receiver<PipeResult>, target: &mut Option<Vec<u8>>) -> Result<(), Error> {
    if target.is_none() {
        match rx.try_recv() {
            Ok(result) => *target = Some(result?),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                return Err(Error::new("execution_io", "native worker output reader stopped"));
            }
        }
    }
    Ok(())
}
fn supervise(command: &mut Command, timeout: Duration, cancellation: Option<&Cancellation>) -> Result<Captured, Error> {
    let stdout =
        io::pipe().map_err(|error| Error::new("execution_io", format!("cannot create worker stdout pipe: {error}")))?;
    let stderr =
        io::pipe().map_err(|error| Error::new("execution_io", format!("cannot create worker stderr pipe: {error}")))?;
    supervise_pipes(command, timeout, cancellation, stdout, stderr)
}
fn supervise_pipes(
    command: &mut Command,
    timeout: Duration,
    cancellation: Option<&Cancellation>,
    stdout: (io::PipeReader, io::PipeWriter),
    stderr: (io::PipeReader, io::PipeWriter),
) -> Result<Captured, Error> {
    let deadline = Instant::now() + timeout;
    let child = command.stdin(Stdio::piped()).stdout(stdout.1).stderr(stderr.1).spawn();
    // Command retains supplied handles for reuse. Drop these copies immediately
    // so only the child can hold our output pipes open.
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let child = child.map_err(|error| Error::new("execution_io", format!("cannot start native worker: {error}")))?;
    let mut worker = Worker { child, lease: None, reaped: false };
    worker.lease = worker.child.stdin.take();
    let stdout = reader(stdout.0, MAX_RESPONSE_BYTES)
        .map_err(|error| worker.error(Error::new("execution_io", format!("cannot read worker stdout: {error}"))))?;
    let stderr = reader(stderr.0, MAX_ERROR_BYTES)
        .map_err(|error| worker.error(Error::new("execution_io", format!("cannot read worker stderr: {error}"))))?;
    let mut output = None;
    let mut errors = None;
    let mut exited = None;
    loop {
        if let Some(cancellation) = cancellation {
            match cancellation.try_recv() {
                Ok(Ok(())) => {
                    return Err(
                        worker.error(Error::new("execution_cancelled", "stdin closed; native worker cancelled"))
                    );
                }
                Ok(Err(error)) => {
                    return Err(
                        worker.error(Error::new("execution_io", format!("cannot read cancellation input: {error}")))
                    );
                }
                Err(TryRecvError::Disconnected) => {
                    return Err(worker.error(Error::new("execution_io", "cancellation reader stopped")));
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        receive(&stdout, &mut output)
            .and_then(|()| receive(&stderr, &mut errors))
            .map_err(|error| worker.error(error))?;
        if exited.is_none() {
            match worker.child.try_wait() {
                Ok(Some(status)) => {
                    worker.reaped = true;
                    worker.lease.take();
                    exited = Some((status, Instant::now()));
                    // Aborts/signals cannot be trusted to leave a valid protocol response.
                    if !status.success() && status.code() != Some(2) {
                        return Err(
                            worker.error(Error::new("worker_failed", format!("native worker terminated: {status}")))
                        );
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    return Err(worker.error(Error::new("execution_io", format!("cannot poll native worker: {error}"))));
                }
            }
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(worker.error(Error::new(
                "execution_timeout",
                format!("native worker exceeded {} ms wall-clock deadline", timeout.as_millis()),
            )));
        }
        if let Some((status, at)) = exited {
            if let (Some(stdout), Some(stderr)) = (output.as_mut(), errors.as_mut()) {
                return Ok(Captured { status, stdout: std::mem::take(stdout), stderr: std::mem::take(stderr) });
            }
            if now.duration_since(at) >= PIPE_GRACE {
                return Err(worker.error(Error::new(
                    "worker_output",
                    "native worker exited but output pipes did not close within the drain grace",
                )));
            }
        }
        thread::sleep(POLL.min(deadline.saturating_duration_since(Instant::now())));
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FailureEnvelope {
    schema_version: u32,
    error: WorkerError,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerError {
    code: String,
    message: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    query_index: Option<usize>,
    #[serde(default)]
    worker_reaped: Option<bool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuccessEnvelope<T = Value> {
    schema_version: u32,
    command: String,
    snapshot: Snapshot,
    capabilities: Value,
    result: T,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    id: String,
    identity: String,
    manifest: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HelpEnvelope {
    schema_version: u32,
    version: String,
    capabilities: Value,
    usage: Vec<String>,
    execution_options: Vec<String>,
    documentation: String,
}
fn valid_success<T: DeserializeOwned>(bytes: &[u8], args: &[String], valid_result: fn(&T) -> bool) -> bool {
    serde_json::from_slice::<SuccessEnvelope<T>>(bytes).is_ok_and(|envelope| {
        envelope.schema_version == 1
            && Some(&envelope.command) == args.first()
            && !envelope.snapshot.id.is_empty()
            && envelope.snapshot.identity == "invocation_local"
            && !envelope.snapshot.manifest.is_empty()
            && envelope.capabilities.is_object()
            && valid_result(&envelope.result)
    })
}
fn validate(captured: Captured, args: &[String]) -> Result<Response, Error> {
    let valid = if captured.status.success() {
        captured.stderr.is_empty()
            && captured.stdout.ends_with(b"\n")
            && if args == ["--help"] || args == ["--version"] {
                serde_json::from_slice::<HelpEnvelope>(&captured.stdout).is_ok_and(|envelope| {
                    envelope.schema_version == 1
                        && envelope.version == env!("CARGO_PKG_VERSION")
                        && envelope.capabilities.is_object()
                        && !envelope.usage.is_empty()
                        && !envelope.execution_options.is_empty()
                        && envelope.documentation == "docs/CLI.md"
                })
            } else {
                match args.first().map(String::as_str) {
                    Some(super::editor::INSPECT_COMMAND) => {
                        valid_success(&captured.stdout, args, super::editor::InspectResult::valid)
                    }
                    Some(super::editor::DEFINITION_COMMAND) => {
                        valid_success(&captured.stdout, args, super::editor::DefinitionResult::valid)
                    }
                    _ => valid_success(&captured.stdout, args, Value::is_object),
                }
            }
    } else {
        captured.status.code() == Some(2)
            && captured.stdout.is_empty()
            && captured.stderr.ends_with(b"\n")
            && serde_json::from_slice::<FailureEnvelope>(&captured.stderr).is_ok_and(|envelope| {
                let error = envelope.error;
                // Typed deserialization rejects duplicate/unknown protocol fields.
                let _ = (error.path, error.query_index, error.worker_reaped);
                envelope.schema_version == 1
                    && !error.message.is_empty()
                    && [
                        "usage",
                        "io",
                        "invalid_path",
                        "invalid_json",
                        "unsupported_version",
                        "invalid_configuration",
                        "document_limit",
                        "request_limit",
                        "unknown_compilation",
                        "load_failed",
                        "invisible_source",
                        "invalid_position",
                        "invalid_target",
                        "invalid_overlay",
                        "overlay_limit",
                        "output_limit",
                        "output_io",
                        "execution_io",
                    ]
                    .contains(&error.code.as_str())
            })
    };
    if valid {
        Ok(Response {
            success: captured.status.success(),
            bytes: if captured.status.success() { captured.stdout } else { captured.stderr },
        })
    } else {
        let mut error =
            Error::new("worker_output", "native worker returned a missing, malformed or contradictory JSON envelope");
        error.worker_reaped = Some(true);
        Err(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write};

    const FIXTURE: &str = "JAVA_ANALYZER_SUPERVISION_TEST";
    fn fixture(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "execution::tests::fixture_process", "--nocapture"]);
        command.env(FIXTURE, mode);
        command
    }

    // Fault injection exists only in this test harness, never in the product binary.
    #[test]
    fn fixture_process() {
        let Ok(mode) = std::env::var(FIXTURE) else { return };
        match mode.as_str() {
            "hang" => thread::sleep(Duration::from_secs(20)),
            "abort" => std::process::abort(),
            "exit7" => std::process::exit(7),
            "empty2" => std::process::exit(2),
            "stdout_limit" => {
                let _ = io::stdout().write_all(&vec![b'x'; MAX_RESPONSE_BYTES + 8192]);
            }
            "stderr_limit" => {
                let _ = io::stderr().write_all(&vec![b'x'; MAX_ERROR_BYTES + 8192]);
            }
            "dual" => {
                let output = thread::spawn(|| io::stdout().write_all(&vec![b'o'; 256 * 1024]).unwrap());
                io::stderr().write_all(&vec![b'e'; 256 * 1024]).unwrap();
                output.join().unwrap();
            }
            "hold" => thread::sleep(Duration::from_secs(20)),
            "lease" => {
                watch_parent().unwrap();
                if let Ok(path) = std::env::var("JAVA_ANALYZER_TEST_PID_FILE") {
                    fs::write(path, std::process::id().to_string()).unwrap();
                }
                thread::sleep(Duration::from_secs(20));
            }
            "supervisor" => {
                supervise(&mut fixture("lease"), Duration::from_secs(10), None).ok();
            }
            _ => panic!("unknown test fixture"),
        }
        std::process::exit(0);
    }

    #[test]
    fn deterministic_timeout_and_cancel_kill_and_reap_direct_worker() {
        let started = Instant::now();
        let error = supervise(&mut fixture("hang"), Duration::from_millis(100), None).err().unwrap();
        assert_eq!(error.code, "execution_timeout");
        assert_eq!(error.worker_reaped, Some(true));
        assert!(started.elapsed() < Duration::from_secs(3));
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(Ok(())).unwrap();
        let error = supervise(&mut fixture("hang"), Duration::from_secs(5), Some(&rx)).err().unwrap();
        assert_eq!(error.code, "execution_cancelled");
        assert_eq!(error.worker_reaped, Some(true));
    }

    #[test]
    fn abort_and_nonprotocol_exit_are_recoverable_worker_failures() {
        for mode in ["abort", "exit7"] {
            let error = supervise(&mut fixture(mode), Duration::from_secs(5), None).err().unwrap();
            assert_eq!(error.code, "worker_failed", "{mode}: {error:?}");
            assert_eq!(error.worker_reaped, Some(true));
        }
    }

    #[test]
    fn overlarge_stdout_and_stderr_are_bounded_before_accumulation() {
        for mode in ["stdout_limit", "stderr_limit"] {
            let error = supervise(&mut fixture(mode), Duration::from_secs(5), None).err().unwrap();
            assert_eq!(error.code, "worker_output_limit", "{mode}: {error:?}");
            assert_eq!(error.worker_reaped, Some(true));
        }
        assert_eq!(read_bounded(&b"12345"[..], 5).unwrap(), b"12345");
        assert_eq!(read_bounded(&b"123456"[..], 5).unwrap_err().code, "worker_output_limit");
    }

    #[test]
    fn simultaneous_pipes_are_drained_without_deadlock() {
        let captured = supervise(&mut fixture("dual"), Duration::from_secs(5), None).ok().unwrap();
        assert!(captured.status.success());
        assert!(captured.stdout.len() >= 256 * 1024);
        assert_eq!(captured.stderr.len(), 256 * 1024);
    }

    #[test]
    fn inherited_pipe_does_not_extend_deadline_or_cleanup() {
        // A separately owned native holder simulates inherited output handles.
        // Keeping its Child lets the test kill and reap it instead of orphaning it.
        let stdout = io::pipe().unwrap();
        let stderr = io::pipe().unwrap();
        let holder = fixture("hold")
            .stdin(Stdio::null())
            .stdout(stdout.1.try_clone().unwrap())
            .stderr(stderr.1.try_clone().unwrap())
            .spawn()
            .unwrap();
        let mut holder = Worker { child: holder, lease: None, reaped: false };
        let started = Instant::now();
        let result = supervise_pipes(&mut fixture("empty2"), Duration::from_secs(5), None, stdout, stderr);
        let elapsed = started.elapsed();
        assert!(holder.stop());
        let error = result.err().unwrap();
        assert_eq!(error.code, "worker_output");
        assert_eq!(error.worker_reaped, Some(true));
        assert!(elapsed < Duration::from_secs(1), "waited for inherited pipe holder");
    }

    #[test]
    fn private_worker_exits_when_lease_closes() {
        let mut child =
            fixture("lease").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        drop(child.stdin.take());
        let mut worker = Worker { child, lease: None, reaped: false };
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = worker.child.try_wait().unwrap() {
                worker.reaped = true;
                assert_eq!(status.code(), Some(2));
                break;
            }
            assert!(Instant::now() < deadline, "worker did not observe closed lease");
            thread::sleep(POLL);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn killing_supervisor_closes_lease_and_stops_worker() {
        use std::{
            path::PathBuf,
            sync::atomic::{AtomicUsize, Ordering},
        };

        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "java-analyzer-lease-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut child = fixture("supervisor")
            .env("JAVA_ANALYZER_TEST_PID_FILE", &path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let pid = loop {
            if let Ok(value) = fs::read_to_string(&path)
                && let Ok(pid) = value.parse::<u32>()
            {
                break pid;
            }
            assert!(Instant::now() < deadline, "worker did not start");
            thread::sleep(POLL);
        };
        fn state(pid: u32) -> Option<u8> {
            match fs::read_to_string(PathBuf::from(format!("/proc/{pid}/stat"))) {
                Ok(value) => Some(value.rsplit_once(") ").expect("valid proc stat").1.as_bytes()[0]),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => panic!("cannot inspect worker process: {error}"),
            }
        }
        assert!(state(pid).is_some_and(|state| state != b'Z'), "worker must be observably running before parent death");
        child.kill().unwrap(); // SIGKILL on Unix: no supervisor cleanup can execute.
        let mut parent = Worker { child, lease: None, reaped: false };
        assert!(parent.stop());
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            // An orphan can be a zombie until the system's reaper collects it.
            // This proves termination, not that the dead supervisor reaped it.
            if state(pid).is_none_or(|state| state == b'Z') {
                break;
            }
            assert!(Instant::now() < deadline, "worker {pid} survived supervisor death");
            thread::sleep(POLL);
        }
        fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    fn status(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }
    #[cfg(windows)]
    fn status(code: i32) -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(code as u32)
    }
    #[test]
    fn editor_option_values_are_not_consumed_as_supervision_flags() {
        for option in ["--overlays", "--line", "--character", "--job"] {
            for value in ["--timeout-ms", "--cancel-on-stdin-close"] {
                let args = vec!["inspect".to_owned(), option.to_owned(), value.to_owned()];
                let parsed = Options::parse(args.clone()).unwrap();
                assert_eq!(parsed.args, args);
                assert_eq!(parsed.timeout, Duration::from_millis(DEFAULT_TIMEOUT_MS));
                assert!(!parsed.cancel_on_stdin_close);
            }
        }
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn overlay_error_codes_survive_the_supervisor_protocol_validator() {
        for code in ["invalid_overlay", "overlay_limit"] {
            let bytes = (json!({"schema_version":1,"error":{"code":code,"message":"invalid buffer","path":"/A.java"}})
                .to_string()
                + "\n")
                .into_bytes();
            let response =
                validate(Captured { status: status(2), stdout: vec![], stderr: bytes.clone() }, &["inspect".into()])
                    .unwrap();
            assert!(!response.success);
            assert_eq!(response.bytes, bytes);
        }
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn private_definition_envelopes_require_typed_exact_fields_and_sound_locations() {
        let command = super::super::editor::DEFINITION_COMMAND;
        let args = vec![command.to_owned()];
        let success = json!({"schema_version":1,"command":command,"snapshot":{"id":"one","identity":"invocation_local","manifest":"/project.json"},"capabilities":{},"result":{"generation":1,"status":"Resolved","refusal":null,"location":{"path":"/A.java","range":{"start":{"line":1,"character":2},"end":{"line":1,"character":3}}}}});
        for (pointer, value) in [
            ("/result/generation", json!(-1)),
            ("/result/generation", json!(9007199254740992_u64)),
            ("/result/status", json!("invented")),
            ("/result/status", json!("Ambiguous")),
            ("/result/refusal", json!("binary_source_unavailable")),
            ("/result/location/path", json!("relative")),
            ("/result/location/range/start/character", json!(4)),
            ("/result/location/range/end/line", json!(-1)),
        ] {
            let mut changed = success.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            let bytes = (changed.to_string() + "\n").into_bytes();
            assert_eq!(
                validate(Captured { status: status(0), stdout: bytes, stderr: vec![] }, &args).err().unwrap().code,
                "worker_output"
            );
        }
        for field in ["generation", "status", "refusal", "location"] {
            let mut changed = success.clone();
            changed["result"].as_object_mut().unwrap().remove(field);
            let bytes = (changed.to_string() + "\n").into_bytes();
            assert_eq!(
                validate(Captured { status: status(0), stdout: bytes, stderr: vec![] }, &args).err().unwrap().code,
                "worker_output"
            );
        }
        let encoded = success.to_string() + "\n";
        for bytes in [
            encoded.replace("\"generation\":1", "\"generation\":1,\"generation\":1"),
            encoded.replace("\"character\":2", "\"character\":2,\"extra\":0"),
            encoded.replace("\"character\":2", "\"character\":2,\"character\":2"),
        ] {
            assert_eq!(
                validate(Captured { status: status(0), stdout: bytes.into_bytes(), stderr: vec![] }, &args)
                    .err()
                    .unwrap()
                    .code,
                "worker_output"
            );
        }
        assert!(
            validate(Captured { status: status(0), stdout: encoded.into_bytes(), stderr: vec![] }, &args)
                .unwrap()
                .success
        );
        let mut refused = success;
        refused["result"]["location"] = Value::Null;
        refused["result"]["refusal"] = json!("binary_source_unavailable");
        assert!(
            validate(
                Captured { status: status(0), stdout: (refused.to_string() + "\n").into_bytes(), stderr: vec![] },
                &args
            )
            .unwrap()
            .success
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn protocol_rejects_missing_multiple_duplicate_wrong_and_contradictory_envelopes() {
        let args = vec!["inspect".to_string()];
        let success = json!({"schema_version":1,"command":"inspect","snapshot":{"id":"one","identity":"invocation_local","manifest":"/project.json"},"capabilities":{},"result":{}}).to_string() + "\n";
        let failure = "{\"schema_version\":1,\"error\":{\"code\":\"io\",\"message\":\"missing\"}}\n";
        for (code, stdout, stderr) in [
            (0, "".to_string(), "".to_string()),
            (0, "garbage\n".to_string(), "".to_string()),
            (0, success.clone() + &success, "".to_string()),
            (0, success.replace("\"schema_version\":1", "\"schema_version\":2"), "".to_string()),
            (0, success.replace("\"schema_version\":1", "\"schema_version\":1,\"schema_version\":1"), "".to_string()),
            (0, success.replace("inspect", "declaration"), "".to_string()),
            (0, success.trim_end().to_string(), "".to_string()),
            (0, success.clone(), "panic\n".to_string()),
            (0, "".to_string(), failure.to_string()),
            (2, success.clone(), "".to_string()),
            (2, "prefix".to_string(), failure.to_string()),
            (2, "".to_string(), failure.replace("\"io\"", "\"unknown_code\"")),
            (2, "".to_string(), "".to_string()),
        ] {
            let result = validate(
                Captured { status: status(code), stdout: stdout.into_bytes(), stderr: stderr.into_bytes() },
                &args,
            );
            assert_eq!(result.err().unwrap().code, "worker_output");
        }
        assert!(
            validate(Captured { status: status(0), stdout: success.into_bytes(), stderr: vec![] }, &args)
                .unwrap()
                .success
        );
        assert!(
            !validate(Captured { status: status(2), stdout: vec![], stderr: failure.as_bytes().to_vec() }, &args)
                .unwrap()
                .success
        );
    }
}
