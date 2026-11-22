//! Log routing for the long-lived `codex-mcp` server.
//!
//! Codex polls about once a second, so any warning raised while rendering (a
//! field Codex never supplies, a slow render, a malformed snapshot field) would
//! otherwise repeat every second in Codex's log. This subscriber writes each
//! distinct line once, and again only after [`REPEAT_AFTER`] if it is still
//! happening. Timestamps and ANSI colors are omitted: identical events must
//! produce identical lines, and Codex records stderr in its own log with its
//! own timestamps.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tracing_subscriber::fmt::MakeWriter;

/// How long an identical line stays suppressed.
const REPEAT_AFTER: Duration = Duration::from_secs(300);
/// Upper bound on remembered lines; the memory is cleared when it is exceeded.
const MAX_REMEMBERED: usize = 512;

/// Build the subscriber used for the whole `codex-mcp` session. Respects
/// `RUST_LOG` like the regular subscriber.
pub(crate) fn subscriber() -> impl tracing::Subscriber + Send + Sync {
    tracing_subscriber::fmt()
        .with_writer(DedupWriter::new(std::io::stderr))
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .without_time()
        .with_ansi(false)
        .finish()
}

/// A `MakeWriter` that drops lines it has written within [`REPEAT_AFTER`].
#[derive(Clone)]
pub(crate) struct DedupWriter<M> {
    inner: M,
    seen: Arc<Mutex<HashMap<Vec<u8>, Instant>>>,
}

impl<M> DedupWriter<M> {
    pub(crate) fn new(inner: M) -> Self {
        Self {
            inner,
            seen: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// True when `line` should be written now (and records that it was).
    fn admit(&self, line: &[u8]) -> bool {
        let Ok(mut seen) = self.seen.lock() else {
            return true;
        };
        let now = Instant::now();
        if let Some(at) = seen.get(line)
            && now.duration_since(*at) < REPEAT_AFTER
        {
            return false;
        }
        if seen.len() >= MAX_REMEMBERED {
            seen.clear();
        }
        seen.insert(line.to_vec(), now);
        true
    }
}

/// One event's writer. `tracing_subscriber::fmt` formats an event into a
/// buffer and writes it in one call, so each `write` is a whole line.
pub(crate) struct DedupLine<'a, M, W> {
    owner: &'a DedupWriter<M>,
    inner: W,
}

impl<'a, M> MakeWriter<'a> for DedupWriter<M>
where
    M: MakeWriter<'a> + 'a,
{
    type Writer = DedupLine<'a, M, M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        DedupLine {
            owner: self,
            inner: self.inner.make_writer(),
        }
    }
}

impl<M, W: Write> Write for DedupLine<'_, M, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.owner.admit(buf) {
            self.inner.write_all(buf)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `MakeWriter` that appends to a shared buffer.
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Capture {
        type Writer = Capture;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn test_repeated_warning_is_written_once() {
        let capture = Capture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(DedupWriter::new(capture.clone()))
            .without_time()
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            for _ in 0..5 {
                tracing::warn!("cship.vim: vim absent from context");
            }
            tracing::warn!("something different");
            tracing::warn!("cship.vim: vim absent from context");
        });
        let out = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        assert_eq!(out.matches("vim absent").count(), 1, "{out}");
        assert_eq!(out.matches("something different").count(), 1, "{out}");
    }

    #[test]
    fn test_memory_is_bounded() {
        let writer = DedupWriter::new(Capture::default());
        for i in 0..(MAX_REMEMBERED + 10) {
            assert!(writer.admit(format!("line {i}\n").as_bytes()));
        }
        assert!(writer.seen.lock().unwrap().len() <= MAX_REMEMBERED);
    }
}
