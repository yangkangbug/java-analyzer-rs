//! The persistent broker invokes only its own one-shot native supervisor.
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const WORK_BUDGET: Duration = Duration::from_millis(30_500);
const CANCEL_GRACE: Duration = Duration::from_millis(750);
const PIPE_GRACE: Duration = Duration::from_millis(100);
const CLEANUP_GRACE: Duration = Duration::from_millis(250);
const MAX_STDOUT: usize = 16 * 1024 * 1024;
const MAX_STDERR: usize = 1024 * 1024;
const MAX_JOB: usize = 4 * 1024 * 1024;

type Capture = Result<Vec<u8>, String>;

pub(super) enum Job {
    Native(Box<NativeJob>),
    #[cfg(test)]
    Fixture(std::sync::Arc<std::sync::Mutex<Barrier>>),
}
impl Job {
    pub fn inspect(manifest: &Path) -> Result<Self, String> {
        NativeJob::inspect(manifest).map(|job| Self::Native(Box::new(job)))
    }
    pub fn definition(files: &mut Files, value: &Value) -> Result<Self, String> {
        NativeJob::definition(files, value).map(|job| Self::Native(Box::new(job)))
    }
    pub fn cancel(&mut self) {
        match self {
            Self::Native(job) => job.cancel(),
            #[cfg(test)]
            Self::Fixture(barrier) => barrier.lock().unwrap().cancelled = true,
        }
    }
    pub fn poll(&mut self) -> Result<Option<Value>, String> {
        match self {
            Self::Native(job) => job.poll(),
            #[cfg(test)]
            Self::Fixture(barrier) => {
                let mut barrier = barrier.lock().unwrap();
                if let Some(error) = barrier.error.take() { Err(error) } else { Ok(barrier.result.take()) }
            }
        }
    }
    #[cfg(test)]
    pub fn fixture(barrier: std::sync::Arc<std::sync::Mutex<Barrier>>) -> Self {
        {
            let mut state = barrier.lock().unwrap();
            state.starts += 1;
            state.active += 1;
            state.maximum_active = state.maximum_active.max(state.active);
            state.cancelled = false;
        }
        Self::Fixture(barrier)
    }
}
#[cfg(test)]
impl Drop for Job {
    fn drop(&mut self) {
        if let Self::Fixture(barrier) = self {
            barrier.lock().unwrap().active -= 1;
        }
    }
}
#[cfg(test)]
#[derive(Default)]
pub(super) struct Barrier {
    pub result: Option<Value>,
    pub error: Option<String>,
    pub cancelled: bool,
    pub starts: usize,
    pub active: usize,
    pub maximum_active: usize,
}

pub(super) struct Files {
    directory: PathBuf,
    next: u64,
}
impl Files {
    pub fn new() -> Result<Self, String> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
        for counter in 0..32 {
            let directory =
                std::env::temp_dir().join(format!("java-analyzer-lsp-{}-{stamp}-{counter}", std::process::id()));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&directory) {
                Ok(()) => return Ok(Self { directory, next: 0 }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(format!("cannot create private LSP input directory: {error}")),
            }
        }
        Err("cannot create unique private LSP input directory".into())
    }
    fn write(&mut self, value: &Value) -> Result<PathBuf, String> {
        let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_JOB {
            return Err("private editor job exceeds 4 MiB".into());
        }
        self.next = self.next.checked_add(1).ok_or("request file counter exhausted")?;
        let path = self.directory.join(format!("job-{}.json", self.next));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        Ok(path)
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

pub(super) struct NativeJob {
    child: Child,
    lease: Option<ChildStdin>,
    stdout: Receiver<Capture>,
    stderr: Receiver<Capture>,
    output: Option<Vec<u8>>,
    errors: Option<Vec<u8>>,
    status: Option<ExitStatus>,
    exited: Option<Instant>,
    started: Instant,
    cancelled: Option<Instant>,
    input: Option<PathBuf>,
    command: &'static str,
}
impl NativeJob {
    pub fn inspect(manifest: &Path) -> Result<Self, String> {
        Self::spawn(super::super::editor::INSPECT_COMMAND, "--manifest", manifest, None)
    }
    pub fn definition(files: &mut Files, value: &Value) -> Result<Self, String> {
        let path = files.write(value)?;
        match Self::spawn(super::super::editor::DEFINITION_COMMAND, "--job", &path, Some(path.clone())) {
            Ok(job) => Ok(job),
            Err(error) => {
                let _ = fs::remove_file(path);
                Err(error)
            }
        }
    }
    fn spawn(command: &'static str, option: &str, path: &Path, input: Option<PathBuf>) -> Result<Self, String> {
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut child = Command::new(executable)
            .args([command, option])
            .arg(path)
            .arg("--cancel-on-stdin-close")
            .arg("--timeout-ms")
            .arg("30000")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start native editor job: {e}"))?;
        let lease = child.stdin.take();
        let stdout_pipe = child.stdout.take().ok_or("missing native stdout")?;
        let stderr_pipe = child.stderr.take().ok_or("missing native stderr")?;
        // If a capture thread cannot start, terminate the process before returning.
        let stdout = match reader(stdout_pipe, MAX_STDOUT) {
            Ok(reader) => reader,
            Err(error) => {
                drop(lease);
                let _ = child.kill();
                reap(&mut child);
                return Err(error);
            }
        };
        let stderr = match reader(stderr_pipe, MAX_STDERR) {
            Ok(reader) => reader,
            Err(error) => {
                drop(lease);
                let _ = child.kill();
                reap(&mut child);
                return Err(error);
            }
        };
        Ok(Self {
            child,
            lease,
            stdout,
            stderr,
            output: None,
            errors: None,
            status: None,
            exited: None,
            started: Instant::now(),
            cancelled: None,
            input,
            command,
        })
    }
    pub fn cancel(&mut self) {
        self.lease.take();
        self.cancelled.get_or_insert_with(Instant::now);
    }
    /// `Err` means the transport/process cleanup is uncertain: stop the session.
    /// A completed worker's ordinary analysis error remains a JSON envelope.
    pub fn poll(&mut self) -> Result<Option<Value>, String> {
        receive(&self.stdout, &mut self.output)?;
        receive(&self.stderr, &mut self.errors)?;
        if self.status.is_none()
            && let Some(status) = self.child.try_wait().map_err(|e| e.to_string())?
        {
            self.status = Some(status);
            self.exited = Some(Instant::now());
        }
        if let Some(status) = self.status
            && let (Some(output), Some(errors)) = (&self.output, &self.errors)
        {
            let bytes = match status.code() {
                Some(0) if errors.is_empty() => output,
                Some(2) if output.is_empty() => errors,
                _ => return Err("native supervisor exited unexpectedly or mixed output streams".into()),
            };
            if bytes.last() != Some(&b'\n') {
                return Err("native supervisor omitted a complete JSON envelope".into());
            }
            let value: Value = serde_json::from_slice(bytes).map_err(|_| "native supervisor returned invalid JSON")?;
            if value["schema_version"] != 1 {
                return Err("native supervisor returned wrong schema".into());
            }
            if status.success() {
                if value["command"] != self.command || !value["result"].is_object() || value.get("error").is_some() {
                    return Err("native supervisor returned wrong command/result".into());
                }
            } else if !value["error"].is_object() || value.get("result").is_some() {
                return Err("native supervisor returned invalid error".into());
            }
            if value["error"]["worker_reaped"] == false {
                return Err("native worker cleanup unconfirmed; restart required".into());
            }
            return Ok(Some(value));
        }
        if self.exited.is_some_and(|at| at.elapsed() >= PIPE_GRACE) {
            return Err("native supervisor output did not close; session stopped".into());
        }
        if self.started.elapsed() >= WORK_BUDGET || self.cancelled.is_some_and(|at| at.elapsed() >= CANCEL_GRACE) {
            return Err("native supervisor exceeded its outer deadline; cleanup cannot be certified".into());
        }
        Ok(None)
    }
}
impl Drop for NativeJob {
    fn drop(&mut self) {
        self.lease.take();
        if self.status.is_none() {
            // Give the existing supervisor time to report and reap its worker.
            let deadline = Instant::now() + CANCEL_GRACE;
            while Instant::now() < deadline {
                match self.child.try_wait() {
                    Ok(Some(status)) => {
                        self.status = Some(status);
                        break;
                    }
                    Ok(None) => thread::sleep(Duration::from_millis(5)),
                    Err(_) => break,
                }
            }
            if self.status.is_none() {
                let _ = self.child.kill();
                reap(&mut self.child);
            }
        }
        if let Some(path) = &self.input {
            let _ = fs::remove_file(path);
        }
    }
}
fn reap(child: &mut Child) {
    let deadline = Instant::now() + CLEANUP_GRACE;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            _ => break,
        }
    }
}
fn reader(mut input: impl Read + Send + 'static, limit: usize) -> Result<Receiver<Capture>, String> {
    let (send, receive) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("lsp-native-capture".into())
        .spawn(move || {
            let result = (|| {
                let mut output = Vec::new();
                let mut buffer = [0; 8192];
                loop {
                    match input.read(&mut buffer) {
                        Ok(0) => return Ok(output),
                        Ok(n) if n <= limit - output.len() => output.extend_from_slice(&buffer[..n]),
                        Ok(_) => return Err("native supervisor exceeded capture limit".into()),
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(error) => return Err(format!("native supervisor capture failed: {error}")),
                    }
                }
            })();
            let _ = send.send(result);
        })
        .map_err(|e| e.to_string())?;
    Ok(receive)
}
fn receive(receiver: &Receiver<Capture>, output: &mut Option<Vec<u8>>) -> Result<(), String> {
    if output.is_none() {
        match receiver.try_recv() {
            Ok(value) => *output = Some(value?),
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => return Err("native output reader stopped".into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_files_are_unique_and_private() {
        let mut files = Files::new().unwrap();
        let one = files.write(&serde_json::json!({"text":"first"})).unwrap();
        let two = files.write(&serde_json::json!({"text":"second"})).unwrap();
        assert_ne!(one, two);
        assert_eq!(fs::read_to_string(&one).unwrap(), "{\"text\":\"first\"}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&one).unwrap().permissions().mode() & 0o777, 0o600);
        }
        drop(files);
        assert!(!one.exists());
    }
    #[cfg(any(unix, windows))]
    #[test]
    fn actual_native_output_validation_refuses_unconfirmed_worker_cleanup() {
        fn fixture(reaped: bool) -> NativeJob {
            // Only the native Rust test executable is launched, already exited
            // before injecting the deterministic captured-envelope fixture.
            let mut child = Command::new(std::env::current_exe().unwrap())
                .arg("--list")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            assert!(child.wait().unwrap().success());
            #[cfg(unix)]
            let status = {
                use std::os::unix::process::ExitStatusExt;
                ExitStatus::from_raw(2 << 8)
            };
            #[cfg(windows)]
            let status = {
                use std::os::windows::process::ExitStatusExt;
                ExitStatus::from_raw(2)
            };
            let (out, stdout) = mpsc::sync_channel(1);
            out.send(Ok(Vec::new())).unwrap();
            let (err, stderr) = mpsc::sync_channel(1);
            let mut bytes = serde_json::to_vec(&serde_json::json!({"schema_version":1,"error":{"code":"execution_cancelled","message":"cancelled","worker_reaped":reaped}})).unwrap();
            bytes.push(b'\n');
            err.send(Ok(bytes)).unwrap();
            NativeJob {
                child,
                lease: None,
                stdout,
                stderr,
                output: None,
                errors: None,
                status: Some(status),
                exited: Some(Instant::now()),
                started: Instant::now(),
                cancelled: None,
                input: None,
                command: super::super::super::editor::DEFINITION_COMMAND,
            }
        }
        assert!(fixture(false).poll().unwrap_err().contains("cleanup unconfirmed"));
        assert_eq!(fixture(true).poll().unwrap().unwrap()["error"]["worker_reaped"], true);
    }
}
