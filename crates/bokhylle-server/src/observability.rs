use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

use tracing_subscriber::fmt::MakeWriter;

pub const REDACTED: &str = "[redacted]";
const MAX_LOG_LINES: usize = 500;

#[derive(Clone, Default)]
pub struct LogHub {
    inner: Arc<Mutex<LogHubInner>>,
}

#[derive(Default)]
struct LogHubInner {
    current: String,
    lines: VecDeque<String>,
}

impl LogHub {
    fn push_fragment(&self, data: &[u8]) {
        let text = String::from_utf8_lossy(data);
        let mut inner = self.inner.lock().expect("log hub lock");

        for character in text.chars() {
            if character == '\n' {
                let line = std::mem::take(&mut inner.current);
                if !line.trim().is_empty() {
                    if inner.lines.len() == MAX_LOG_LINES {
                        inner.lines.pop_front();
                    }
                    inner.lines.push_back(line);
                }
            } else if character != '\r' {
                inner.current.push(character);
            }
        }
    }

    pub fn recent(&self, limit: usize) -> Vec<String> {
        let inner = self.inner.lock().expect("log hub lock");
        inner
            .lines
            .iter()
            .rev()
            .take(limit.clamp(1, MAX_LOG_LINES))
            .cloned()
            .collect()
    }
}

impl Write for LogHub {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.push_fragment(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for LogHub {
    type Writer = LogHub;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[derive(Clone)]
struct TeeWriter {
    hub: LogHub,
}

impl Write for TeeWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stdout().write_all(data);
        self.hub.push_fragment(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stdout().flush()
    }
}

impl<'a> MakeWriter<'a> for TeeWriter {
    type Writer = TeeWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

pub fn log_hub() -> &'static LogHub {
    static HUB: OnceLock<LogHub> = OnceLock::new();
    HUB.get_or_init(LogHub::default)
}

pub fn recent_logs(limit: usize) -> Vec<String> {
    log_hub().recent(limit)
}

pub fn init() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("BOKHYLLE_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let json = std::env::var("BOKHYLLE_LOG_FORMAT")
        .map(|value| value.eq_ignore_ascii_case("json"))
        .unwrap_or(false);

    let writer = TeeWriter {
        hub: log_hub().clone(),
    };
    // Custom writers bypass tracing-subscriber's terminal detection, so decide
    // colouring explicitly; the log store always stays free of ANSI codes.
    let ansi = std::io::IsTerminal::is_terminal(&std::io::stdout());

    if json {
        tracing_subscriber::fmt()
            .json()
            .with_writer(writer)
            .with_ansi(ansi)
            .with_env_filter(filter)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_writer(writer)
            .with_ansi(ansi)
            .with_env_filter(filter)
            .init();
    }
}

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Buffer {
    fn contents(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("log buffer lock")).into_owned()
    }
}

impl Write for Buffer {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("log buffer lock")
            .extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Buffer {
    type Writer = Buffer;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

pub struct LogCapture {
    buffer: Buffer,
    _guard: tracing::subscriber::DefaultGuard,
}

impl LogCapture {
    pub fn new() -> Self {
        let buffer = Buffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(buffer.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);
        Self {
            buffer,
            _guard: guard,
        }
    }

    pub fn logs(&self) -> String {
        self.buffer.contents()
    }
}

impl Default for LogCapture {
    fn default() -> Self {
        Self::new()
    }
}
