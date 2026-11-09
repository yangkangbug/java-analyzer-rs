//! Bounded LSP framing. No analysis and no filesystem access.
use serde::Deserialize;
use serde_json::Value;
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

pub(super) const MAX_HEADER: usize = 8 * 1024;
pub(super) const MAX_BODY: usize = 2 * 1024 * 1024;
pub(super) const MAX_OUTPUT: usize = 16 * 1024 * 1024;
const WRITER_STALL: Duration = Duration::from_secs(5);

pub(super) enum Incoming {
    Message(Result<Value, String>),
    End(Result<(), String>),
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(super) fn read_frame(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut used = 0_usize;
    let mut length = None;
    loop {
        let mut line = Vec::new();
        loop {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                if used == 0 && line.is_empty() {
                    return Ok(None);
                }
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF inside LSP header"));
            }
            let count = available.iter().position(|b| *b == b'\n').map_or(available.len(), |n| n + 1);
            if count > MAX_HEADER - used {
                return Err(invalid("LSP header exceeds 8 KiB"));
            }
            used += count;
            line.extend_from_slice(&available[..count]);
            reader.consume(count);
            if line.last() == Some(&b'\n') {
                break;
            }
        }
        if !line.ends_with(b"\r\n") || !line.is_ascii() {
            return Err(invalid("LSP headers must be ASCII and CRLF terminated"));
        }
        line.truncate(line.len() - 2);
        if line.is_empty() {
            break;
        }
        let line = std::str::from_utf8(&line).map_err(|_| invalid("invalid header"))?;
        let (name, value) = line.split_once(':').ok_or_else(|| invalid("invalid LSP header"))?;
        let value = value.trim();
        if name.eq_ignore_ascii_case("Content-Length") {
            if length.is_some() || value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid("duplicate or invalid Content-Length"));
            }
            let count = value.parse::<usize>().map_err(|_| invalid("invalid Content-Length"))?;
            if count > MAX_BODY {
                return Err(invalid("LSP body exceeds 2 MiB"));
            }
            length = Some(count);
        } else if name.eq_ignore_ascii_case("Content-Type") {
            let value = value.to_ascii_lowercase();
            if let Some((_, encoding)) = value.split_once("charset=")
                && !["utf-8", "utf8"].contains(&encoding.trim())
            {
                return Err(invalid("unsupported LSP character encoding"));
            }
        }
    }
    let length = length.ok_or_else(|| invalid("missing Content-Length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

// Typed envelope parsing rejects duplicate important fields while allowing
// standard client extensions. The subsequent Value keeps null-vs-absent IDs.
#[derive(Deserialize)]
struct Envelope {
    jsonrpc: Value,
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    method: Option<Value>,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<Value>,
}
fn decode(body: &[u8]) -> Result<Value, String> {
    let envelope: Envelope = serde_json::from_slice(body).map_err(|_| "invalid JSON-RPC object".to_owned())?;
    let _ = (envelope.jsonrpc, envelope.id, envelope.method, envelope.params, envelope.result, envelope.error);
    serde_json::from_slice(body).map_err(|_| "invalid JSON-RPC object".to_owned())
}

struct Outgoing {
    body: Vec<u8>,
    guard: Option<(u64, Arc<AtomicU64>)>,
    obsolete: Vec<u8>,
    log: Vec<u8>,
}

pub(super) struct Transport {
    pub incoming: Receiver<Incoming>,
    output: SyncSender<Outgoing>,
    bytes: Arc<AtomicUsize>,
    writing: Arc<Mutex<Option<Instant>>>,
    failure: Receiver<String>,
}
impl Transport {
    #[cfg(test)]
    pub fn fixture() -> (Self, Arc<Mutex<Vec<u8>>>) {
        struct Shared(Arc<Mutex<Vec<u8>>>);
        impl Write for Shared {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let bytes = Arc::new(Mutex::new(Vec::new()));
        (Self::new(io::empty(), Shared(bytes.clone())).unwrap(), bytes)
    }
    pub fn stdio() -> io::Result<Self> {
        Self::new(io::stdin(), io::stdout())
    }
    fn new(input: impl Read + Send + 'static, output: impl Write + Send + 'static) -> io::Result<Self> {
        let (send, incoming) = mpsc::sync_channel(8);
        thread::Builder::new().name("lsp-input".into()).spawn(move || {
            let mut input = BufReader::new(input);
            loop {
                let event = match read_frame(&mut input) {
                    Ok(Some(body)) => Incoming::Message(decode(&body)),
                    Ok(None) => Incoming::End(Ok(())),
                    Err(error) => Incoming::End(Err(error.to_string())),
                };
                let end = matches!(event, Incoming::End(_));
                if send.send(event).is_err() || end {
                    return;
                }
            }
        })?;
        let (send, receive) = mpsc::sync_channel::<Outgoing>(16);
        let (fail, failure) = mpsc::sync_channel(1);
        let bytes = Arc::new(AtomicUsize::new(0));
        let writing = Arc::new(Mutex::new(None));
        let (worker_bytes, worker_writing) = (bytes.clone(), writing.clone());
        thread::Builder::new().name("lsp-output".into()).spawn(move || {
            let mut output = output;
            while let Ok(message) = receive.recv() {
                let size = message.body.len() + message.obsolete.len() + message.log.len();
                let obsolete = message
                    .guard
                    .as_ref()
                    .is_some_and(|(expected, current)| *expected != current.load(Ordering::SeqCst));
                let body = if obsolete { &message.obsolete } else { &message.body };
                *worker_writing.lock().expect("writer state") = Some(Instant::now());
                let result = write!(output, "Content-Length: {}\r\n\r\n", body.len())
                    .and_then(|_| output.write_all(body))
                    .and_then(|_| {
                        if obsolete {
                            write!(output, "Content-Length: {}\r\n\r\n", message.log.len())
                                .and_then(|_| output.write_all(&message.log))
                        } else {
                            Ok(())
                        }
                    })
                    .and_then(|_| output.flush());
                worker_bytes.fetch_sub(size, Ordering::Relaxed);
                *worker_writing.lock().expect("writer state") = None;
                if let Err(error) = result {
                    let _ = fail.send(error.to_string());
                    return;
                }
            }
        })?;
        Ok(Self { incoming, output: send, bytes, writing, failure })
    }
    pub fn send(&self, value: Value) -> Result<(), String> {
        self.enqueue(value, None)
    }
    pub fn send_guarded(&self, value: Value, expected: u64, current: Arc<AtomicU64>) -> Result<(), String> {
        self.enqueue(value, Some((expected, current)))
    }
    fn enqueue(&self, value: Value, guard: Option<(u64, Arc<AtomicU64>)>) -> Result<(), String> {
        let obsolete = if guard.is_some() {
            serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":value["id"],"result":null}))
                .map_err(|e| e.to_string())?
        } else {
            Vec::new()
        };
        let log = if guard.is_some() {
            serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","method":"window/logMessage","params":{"type":3,"message":"Definition discarded before output because the workspace generation changed"}})).map_err(|e| e.to_string())?
        } else {
            Vec::new()
        };
        let body = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
        if body.len() > MAX_OUTPUT {
            return Err("LSP response exceeds output limit".into());
        }
        let size = body.len() + obsolete.len() + log.len();
        self.bytes
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(size).filter(|n| *n <= MAX_OUTPUT)
            })
            .map_err(|_| "LSP output byte queue saturated".to_owned())?;
        match self.output.try_send(Outgoing { body, guard, obsolete, log }) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.bytes.fetch_sub(size, Ordering::Relaxed);
                Err("LSP output queue saturated".into())
            }
            Err(TrySendError::Disconnected(_)) => {
                self.bytes.fetch_sub(size, Ordering::Relaxed);
                Err("LSP output writer stopped".into())
            }
        }
    }
    pub fn check(&self) -> Result<(), String> {
        if let Ok(error) = self.failure.try_recv() {
            return Err(format!("LSP output failed: {error}"));
        }
        if self
            .writing
            .lock()
            .map_err(|_| "writer state failed")?
            .is_some_and(|started| started.elapsed() >= WRITER_STALL)
        {
            return Err("LSP output stalled for five seconds".into());
        }
        Ok(())
    }
    pub fn drained(&self) -> bool {
        self.bytes.load(Ordering::Relaxed) == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn unicode_lengths_and_coalesced_frames() {
        let text = "{\"text\":\"中😀\"}";
        let bytes = format!(
            "content-length: {}\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{text}Content-Length: 2\r\n\r\n{{}}",
            text.len()
        );
        let mut read = Cursor::new(bytes);
        assert_eq!(read_frame(&mut read).unwrap().unwrap(), text.as_bytes());
        assert_eq!(read_frame(&mut read).unwrap().unwrap(), b"{}");
        assert_eq!(read_frame(&mut read).unwrap(), None);
    }
    #[test]
    fn framing_limits_reject_before_body_allocation() {
        for header in [
            "Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
            "Content-Length: 2097153\r\n\r\n",
            "Content-Length: +2\r\n\r\n{}",
            "X: y\r\n\r\n{}",
            "Content-Length: 2\n\n{}",
            "Content-Length: 3\r\n\r\n{}",
        ] {
            assert!(read_frame(&mut Cursor::new(header)).is_err(), "{header}");
        }
        assert!(read_frame(&mut Cursor::new(vec![b'x'; MAX_HEADER + 1])).is_err());
    }
    #[test]
    fn envelopes_allow_extensions_but_not_duplicate_fields() {
        assert!(decode(br#"{"jsonrpc":"2.0","method":"initialized","newField":true}"#).is_ok());
        assert!(decode(br#"{"jsonrpc":"2.0","id":1,"id":2,"method":"initialize"}"#).is_err());
    }
    struct GateWriter {
        gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
        bytes: Arc<Mutex<Vec<u8>>>,
        entered: Option<SyncSender<()>>,
    }
    impl Write for GateWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if let Some(entered) = self.entered.take() {
                entered.send(()).unwrap();
                let (lock, wake) = &*self.gate;
                let mut open = lock.lock().unwrap();
                while !*open {
                    open = wake.wait(open).unwrap();
                }
            }
            self.bytes.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    type Gated = (Transport, Arc<(Mutex<bool>, std::sync::Condvar)>, Arc<Mutex<Vec<u8>>>);
    fn gated() -> Gated {
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let (entered, event) = mpsc::sync_channel(1);
        let transport = Transport::new(
            io::empty(),
            GateWriter { gate: gate.clone(), bytes: bytes.clone(), entered: Some(entered) },
        )
        .unwrap();
        transport.send(serde_json::json!({"jsonrpc":"2.0","id":0,"result":null})).unwrap();
        event.recv_timeout(Duration::from_secs(1)).unwrap();
        (transport, gate, bytes)
    }
    fn release(gate: &Arc<(Mutex<bool>, std::sync::Condvar)>) {
        let (lock, wake) = &**gate;
        *lock.lock().unwrap() = true;
        wake.notify_all();
    }
    #[test]
    fn queued_location_is_rechecked_before_writer_commit() {
        let (transport, gate, bytes) = gated();
        let generation = Arc::new(AtomicU64::new(1));
        transport
            .send_guarded(
                serde_json::json!({"jsonrpc":"2.0","id":1,"result":{"uri":"file:///old.java","range":{}}}),
                1,
                generation.clone(),
            )
            .unwrap();
        generation.store(2, Ordering::SeqCst);
        release(&gate);
        let end = Instant::now() + Duration::from_secs(1);
        while !transport.drained() && Instant::now() < end {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(transport.drained());
        let mut input = Cursor::new(bytes.lock().unwrap().clone());
        read_frame(&mut input).unwrap();
        let response: Value = serde_json::from_slice(&read_frame(&mut input).unwrap().unwrap()).unwrap();
        assert_eq!(response["id"], 1);
        assert_eq!(response["result"], Value::Null);
        let log: Value = serde_json::from_slice(&read_frame(&mut input).unwrap().unwrap()).unwrap();
        assert_eq!(log["method"], "window/logMessage");
    }
    #[test]
    fn saturated_output_queue_and_stalled_writer_fail_without_waiting_for_join() {
        let (transport, gate, _) = gated();
        for id in 1..=16 {
            transport.send(serde_json::json!({"jsonrpc":"2.0","id":id,"result":null})).unwrap();
        }
        assert!(
            transport
                .send(serde_json::json!({"jsonrpc":"2.0","id":17,"result":null}))
                .unwrap_err()
                .contains("saturated")
        );
        *transport.writing.lock().unwrap() = Some(Instant::now() - WRITER_STALL);
        assert!(transport.check().unwrap_err().contains("stalled"));
        release(&gate);
    }
    #[test]
    fn aggregate_output_bytes_are_bounded_even_with_queue_slots_remaining() {
        let (transport, gate, _) = gated();
        transport.send(serde_json::json!({"result":"x".repeat(MAX_OUTPUT / 2)})).unwrap();
        assert!(
            transport
                .send(serde_json::json!({"result":"x".repeat(MAX_OUTPUT / 2)}))
                .unwrap_err()
                .contains("byte queue")
        );
        release(&gate);
    }
}
