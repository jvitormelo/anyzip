//! N-API binding (Node and Bun). Converts types only; all logic lives in the `anyzip` crate.
//!
//! - Input: the JS `Uint8Array`/`Buffer` is referenced, not copied, for the life of the task. The
//!   caller must not mutate or transfer it until the promise settles.
//! - Output: each entry's `Vec<u8>` becomes a `Buffer` without a copy (external buffer).
//! - Work runs on the libuv thread pool (`AsyncTask`); `extractEach` uses one extra thread per
//!   open iterator, handing entries over one at a time.
//! - Errors resolve as `{ ok: false, code, message }`; the JS wrapper turns them into
//!   `AnyzipError` (napi-rs rejections cannot carry our own `code`).

#![expect(
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    reason = "the API surface is JS: errors and results are documented in npm/anyzip.d.ts"
)]

use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;

use napi::bindgen_prelude::*;
use napi_derive::napi;

/// Entry buffers are large, short-lived and freed on the JS thread, not on the worker that made
/// them. With glibc malloc a burst of extractions left hundreds of MB resident (its mmap threshold
/// grows after the first large free); mimalloc with immediate purging gives the memory back.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// `mi_option_purge_delay` in mimalloc.h (v3; default 1000 ms, which a serverless function may
/// spend idle with the memory still resident).
const MI_OPTION_PURGE_DELAY: libmimalloc_sys::mi_option_t = 15;

#[napi_derive::module_init]
fn init() {
    // SAFETY: sets a plain integer option; mimalloc reads options atomically on each purge.
    unsafe { libmimalloc_sys::mi_option_set(MI_OPTION_PURGE_DELAY, 0) };
}

/// Limits from JS. Absent fields keep the defaults of `anyzip::Limits`.
#[napi(object)]
#[derive(Default)]
pub struct Limits {
    pub max_entries: Option<f64>,
    pub max_entry_bytes: Option<f64>,
    pub max_total_bytes: Option<f64>,
    pub max_ratio: Option<f64>,
    pub max_memory_bytes: Option<f64>,
}

#[napi(object)]
pub struct Entry {
    pub path: String,
    pub data: Buffer,
}

#[napi(object)]
pub struct Skipped {
    pub path: String,
    pub reason: String,
}

#[napi(object)]
pub struct ListedEntry {
    pub path: String,
    pub size: f64,
}

/// `ok: true` carries the result fields; `ok: false` carries `code` and `message`.
#[napi(object)]
pub struct Outcome {
    pub ok: bool,
    pub code: Option<String>,
    pub message: Option<String>,
    pub format: Option<String>,
    pub entries: Option<Vec<Entry>>,
    pub listed: Option<Vec<ListedEntry>>,
    pub skipped: Option<Vec<Skipped>>,
    pub recovered: Option<bool>,
    pub count: Option<u32>,
    pub stopped: Option<bool>,
}

impl Outcome {
    fn empty(ok: bool) -> Self {
        Self {
            ok,
            code: None,
            message: None,
            format: None,
            entries: None,
            listed: None,
            skipped: None,
            recovered: None,
            count: None,
            stopped: None,
        }
    }

    fn error(e: &anyzip::Error) -> Self {
        Self {
            code: Some(e.code.as_str().to_owned()),
            message: Some(e.message.clone()),
            ..Self::empty(false)
        }
    }
}

fn skipped(list: Vec<anyzip::Skipped>) -> Vec<Skipped> {
    list.into_iter()
        .map(|s| Skipped {
            path: s.path,
            reason: s.reason.as_str().to_owned(),
        })
        .collect()
}

fn entry(e: anyzip::Entry) -> Entry {
    Entry {
        path: e.path,
        data: Buffer::from(e.data),
    }
}

fn to_limits(js: Option<Limits>) -> Result<anyzip::Limits> {
    let mut limits = anyzip::Limits::default();
    let Some(js) = js else {
        return Ok(limits);
    };
    let int = |name: &str, v: f64| -> Result<u64> {
        // 2^53: past it a JS number cannot represent every integer.
        if v.is_finite() && v >= 0.0 && v.fract() == 0.0 && v <= 9_007_199_254_740_992.0 {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "checked above"
            )]
            Ok(v as u64)
        } else {
            Err(Error::new(
                Status::InvalidArg,
                format!("limits.{name} must be a non-negative integer"),
            ))
        }
    };
    let small = |name: &str, v: f64| -> Result<u32> {
        u32::try_from(int(name, v)?)
            .map_err(|_| Error::new(Status::InvalidArg, format!("limits.{name} is too large")))
    };
    if let Some(v) = js.max_entries {
        limits.max_entries = small("maxEntries", v)?;
    }
    if let Some(v) = js.max_entry_bytes {
        limits.max_entry_bytes = int("maxEntryBytes", v)?;
    }
    if let Some(v) = js.max_total_bytes {
        limits.max_total_bytes = int("maxTotalBytes", v)?;
    }
    if let Some(v) = js.max_ratio {
        limits.max_ratio = small("maxRatio", v)?;
    }
    if let Some(v) = js.max_memory_bytes {
        limits.max_memory_bytes = int("maxMemoryBytes", v)?;
    }
    Ok(limits)
}

/// Format from the signature, or `null`. Synchronous: it reads at most 8 bytes.
#[napi]
pub fn detect(bytes: &[u8]) -> Option<&'static str> {
    anyzip::detect(bytes).map(anyzip::Format::as_str)
}

pub struct ExtractTask {
    bytes: Uint8Array,
    limits: anyzip::Limits,
}

impl Task for ExtractTask {
    type Output = std::result::Result<anyzip::Extracted, anyzip::Error>;
    type JsValue = Outcome;

    fn compute(&mut self) -> Result<Self::Output> {
        Ok(anyzip::extract(&self.bytes, &self.limits))
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(match output {
            Ok(out) => Outcome {
                format: Some(out.format.as_str().to_owned()),
                entries: Some(out.entries.into_iter().map(entry).collect()),
                skipped: Some(skipped(out.skipped)),
                recovered: Some(out.recovered),
                ..Outcome::empty(true)
            },
            Err(e) => Outcome::error(&e),
        })
    }
}

/// Extracts every regular file. Runs on the libuv thread pool.
#[napi(ts_return_type = "Promise<Outcome>")]
pub fn extract(bytes: Uint8Array, limits: Option<Limits>) -> Result<AsyncTask<ExtractTask>> {
    Ok(AsyncTask::new(ExtractTask {
        bytes,
        limits: to_limits(limits)?,
    }))
}

pub struct ListTask {
    bytes: Uint8Array,
    limits: anyzip::Limits,
}

impl Task for ListTask {
    type Output = std::result::Result<anyzip::Listing, anyzip::Error>;
    type JsValue = Outcome;

    fn compute(&mut self) -> Result<Self::Output> {
        Ok(anyzip::list(&self.bytes, &self.limits))
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(match output {
            Ok(out) => Outcome {
                format: Some(out.format.as_str().to_owned()),
                #[expect(clippy::cast_precision_loss, reason = "sizes above 2^53 do not exist")]
                listed: Some(
                    out.entries
                        .into_iter()
                        .map(|e| ListedEntry {
                            path: e.path,
                            size: e.size as f64,
                        })
                        .collect(),
                ),
                skipped: Some(skipped(out.skipped)),
                recovered: Some(out.recovered),
                ..Outcome::empty(true)
            },
            Err(e) => Outcome::error(&e),
        })
    }
}

/// Lists entries from headers without decompressing.
#[napi(ts_return_type = "Promise<Outcome>")]
pub fn list(bytes: Uint8Array, limits: Option<Limits>) -> Result<AsyncTask<ListTask>> {
    Ok(AsyncTask::new(ListTask {
        bytes,
        limits: to_limits(limits)?,
    }))
}

// ---------------------------------------------------------------------------------------------
// extractEach: pull-based, one entry at a time
// ---------------------------------------------------------------------------------------------

pub enum Message {
    Entry(anyzip::Entry),
    Done(std::result::Result<anyzip::Summary, anyzip::Error>),
}

/// Receiver side of a running extraction. Dropping the receiver makes the worker's next `send`
/// fail, which stops the extraction.
struct Shared {
    receiver: Mutex<Option<Receiver<Message>>>,
    cancelled: AtomicBool,
}

impl Shared {
    fn finish(&self) {
        if let Ok(mut guard) = self.receiver.lock() {
            guard.take();
        }
    }
}

/// An extraction running on its own thread. `next()` hands over one entry; the worker decodes
/// the following entry meanwhile and then waits until it is asked for it (rendezvous channel).
#[napi]
pub struct EachIterator {
    shared: Arc<Shared>,
}

#[napi]
impl EachIterator {
    /// Next entry, or the final summary (`entries` empty, `count` set) when done.
    #[napi(ts_return_type = "Promise<Outcome>")]
    pub fn next(&self) -> AsyncTask<NextTask> {
        AsyncTask::new(NextTask {
            shared: Arc::clone(&self.shared),
        })
    }

    /// Stops the extraction early. Never blocks the JS thread: if a `next()` is waiting for the
    /// worker, that task drops the channel when it wakes up.
    #[napi]
    pub fn close(&self) {
        self.shared.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.shared.receiver.try_lock() {
            guard.take();
        }
    }
}

pub struct NextTask {
    shared: Arc<Shared>,
}

impl Task for NextTask {
    type Output = Option<Message>;
    type JsValue = Outcome;

    fn compute(&mut self) -> Result<Self::Output> {
        let mut guard = self
            .shared
            .receiver
            .lock()
            .map_err(|_| Error::new(Status::GenericFailure, "extractEach state poisoned"))?;
        if self.shared.cancelled.load(Ordering::SeqCst) {
            guard.take();
            return Ok(None);
        }
        let Some(receiver) = guard.as_ref() else {
            return Ok(None);
        };
        let message = receiver.recv().ok();
        if self.shared.cancelled.load(Ordering::SeqCst) {
            guard.take();
            return Ok(None);
        }
        Ok(message)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(match output {
            Some(Message::Entry(e)) => Outcome {
                entries: Some(vec![entry(e)]),
                ..Outcome::empty(true)
            },
            Some(Message::Done(Ok(summary))) => {
                self.shared.finish();
                Outcome {
                    format: Some(summary.format.as_str().to_owned()),
                    entries: Some(Vec::new()),
                    skipped: Some(skipped(summary.skipped)),
                    recovered: Some(summary.recovered),
                    count: Some(u32::try_from(summary.entries).unwrap_or(u32::MAX)),
                    stopped: Some(summary.stopped),
                    ..Outcome::empty(true)
                }
            }
            Some(Message::Done(Err(e))) => {
                self.shared.finish();
                Outcome::error(&e)
            }
            // Closed by the caller (or the worker is gone): report a stopped, empty summary.
            None => Outcome {
                entries: Some(Vec::new()),
                stopped: Some(true),
                ..Outcome::empty(true)
            },
        })
    }
}

/// Starts an extraction that yields entries one at a time; see [`EachIterator`].
#[napi]
pub fn extract_each(bytes: Uint8Array, limits: Option<Limits>) -> Result<EachIterator> {
    let limits = to_limits(limits)?;
    let (sender, receiver): (SyncSender<Message>, Receiver<Message>) = sync_channel(0);
    thread::Builder::new()
        .name("anyzip-each".to_owned())
        .spawn(move || {
            let result = anyzip::extract_each(&bytes, &limits, |e| {
                // The receiver is gone (closed or garbage collected): stop decoding.
                match sender.send(Message::Entry(e)) {
                    Ok(()) => ControlFlow::Continue(()),
                    Err(_) => ControlFlow::Break(()),
                }
            });
            let _ = sender.send(Message::Done(result));
            drop(bytes);
        })
        .map_err(|e| Error::new(Status::GenericFailure, format!("cannot start thread: {e}")))?;
    Ok(EachIterator {
        shared: Arc::new(Shared {
            receiver: Mutex::new(Some(receiver)),
            cancelled: AtomicBool::new(false),
        }),
    })
}
