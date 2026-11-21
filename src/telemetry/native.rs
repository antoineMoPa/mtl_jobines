use opentelemetry::trace::{Span, TraceContextExt, Tracer, TracerProvider};
use opentelemetry::{Context, ContextGuard, KeyValue, Value};
use opentelemetry_sdk::error::{OTelSdkError, OTelSdkResult};
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider, SpanData, SpanExporter};
use serde_json::json;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static ENABLED: AtomicBool = AtomicBool::new(false);
static TRACER: RwLock<Option<SdkTracer>> = RwLock::new(None);
static FRAME: AtomicI64 = AtomicI64::new(0);
static NEXT_THREAD: AtomicI64 = AtomicI64::new(1);
thread_local! { static THREAD: i64 = NEXT_THREAD.fetch_add(1, Ordering::Relaxed); }

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub struct ActiveSpan {
    _guard: ContextGuard,
}
impl Drop for ActiveSpan {
    fn drop(&mut self) {
        Context::current().span().end();
    }
}
pub fn span(name: &'static str) -> Option<ActiveSpan> {
    if !enabled() {
        return None;
    }
    let tracer = TRACER.read().unwrap();
    let mut span = tracer.as_ref()?.start(name);
    span.set_attribute(KeyValue::new("thread", THREAD.with(|id| *id)));
    Some(ActiveSpan {
        _guard: Context::current_with_span(span).attach(),
    })
}

pub fn frame() -> Option<ActiveSpan> {
    let guard = span("game.frame")?;
    Context::current().span().set_attribute(KeyValue::new(
        "frame",
        FRAME.fetch_add(1, Ordering::Relaxed),
    ));
    Some(guard)
}

// Readback may complete several frames later. Keep the submitting trace and
// its timestamp so Perfetto counters line up with the CPU work that issued it.
pub struct GpuSample {
    tracer: SdkTracer,
    parent: Context,
    submitted: SystemTime,
}
impl GpuSample {
    pub fn current() -> Option<Self> {
        Some(Self {
            tracer: TRACER.read().unwrap().clone()?,
            parent: Context::current(),
            submitted: SystemTime::now(),
        })
    }
    pub fn duration(&self, name: &'static str, ms: f64) {
        let mut span = self
            .tracer
            .span_builder(name)
            .with_start_time(self.submitted)
            .start_with_context(&self.tracer, &self.parent);
        span.set_attribute(KeyValue::new("gpu.ms", ms));
        span.end_with_timestamp(self.submitted);
    }
}

pub struct Capture(SdkTracerProvider);
pub fn start(directory: &Path) -> Result<Option<Capture>, String> {
    if std::env::var("MTL_TRACE").as_deref() != Ok("1") {
        return Ok(None);
    }
    let mut tracer = TRACER.write().unwrap();
    if tracer.is_some() {
        return Err("A game trace is already recording".into());
    }
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = directory.join(format!("game-{stamp}-{}.partial", std::process::id()));
    let mut file = BufWriter::new(File::create_new(&path).map_err(|e| e.to_string())?);
    file.write_all(b"[").map_err(|e| e.to_string())?;
    eprintln!(
        "Recording game trace to {}",
        path.with_extension("json").display()
    );
    let exporter = FileExporter {
        path,
        output: Mutex::new(Output {
            file,
            first: true,
            named_threads: HashSet::new(),
            failed: false,
        }),
    };
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .build();
    *tracer = Some(provider.tracer("mtl_jobines"));
    ENABLED.store(true, Ordering::Release);
    Ok(Some(Capture(provider)))
}
impl Drop for Capture {
    fn drop(&mut self) {
        let mut tracer = TRACER.write().unwrap();
        ENABLED.store(false, Ordering::Release);
        *tracer = None;
        if let Err(error) = self.0.shutdown() {
            eprintln!("Game trace shutdown: {error}");
        }
    }
}

#[derive(Debug)]
struct Output {
    file: BufWriter<File>,
    first: bool,
    named_threads: HashSet<i64>,
    failed: bool,
}
#[derive(Debug)]
struct FileExporter {
    path: PathBuf,
    output: Mutex<Output>,
}
impl Output {
    fn event(&mut self, event: &serde_json::Value) -> io::Result<()> {
        if !self.first {
            self.file.write_all(b",\n")?;
        }
        self.first = false;
        serde_json::to_writer(&mut self.file, event)?;
        Ok(())
    }

    fn write(&mut self, spans: Vec<SpanData>) -> io::Result<()> {
        for span in spans {
            let ts = span
                .start_time
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64;
            let mut args = json!({"trace_id":span.span_context.trace_id().to_string(), "span_id":span.span_context.span_id().to_string(), "parent_span_id":span.parent_span_id.to_string()});
            let mut tid = 0;
            let mut gpu = None;
            for attr in span.attributes {
                match (attr.key.as_str(), attr.value) {
                    ("thread", Value::I64(id)) => tid = id,
                    ("frame", Value::I64(id)) => args["frame"] = json!(id),
                    ("gpu.ms", Value::F64(ms)) => gpu = Some(ms),
                    _ => {}
                }
            }
            let thread_name = match span.name.as_ref() {
                "game.frame" | "map.startup.apply" => Some("Game thread"),
                "map.startup.worker" => Some("Map loader"),
                "map.reload.worker" => Some("Map reload"),
                _ => None,
            };
            if let Some(name) = thread_name.filter(|_| self.named_threads.insert(tid)) {
                self.event(
                    &json!({"ph":"M","name":"thread_name","pid":1,"tid":tid,"args":{"name":name}}),
                )?;
            }
            let event = if let Some(ms) = gpu {
                json!({"ph":"C","cat":"GPU","name":span.name,"ts":ts,"pid":1,"tid":0,"args":{"ms":ms}})
            } else {
                let us = span
                    .end_time
                    .duration_since(span.start_time)
                    .unwrap_or_default()
                    .as_micros() as u64;
                json!({"ph":"X","cat":"CPU","name":span.name,"ts":ts,"dur":us,"pid":1,"tid":tid,"args":args})
            };
            self.event(&event)?;
        }
        self.file.flush()
    }
}
impl SpanExporter for FileExporter {
    async fn export(&self, spans: Vec<SpanData>) -> OTelSdkResult {
        let mut output = self.output.lock().unwrap();
        output.write(spans).map_err(|error| {
            if !output.failed {
                eprintln!("Could not write game trace: {error}");
            }
            output.failed = true;
            OTelSdkError::InternalFailure(error.to_string())
        })
    }
    fn shutdown_with_timeout(&self, _: Duration) -> OTelSdkResult {
        let mut output = self.output.lock().unwrap();
        if output.failed {
            return Err(OTelSdkError::InternalFailure(
                "Incomplete game trace".into(),
            ));
        }
        output
            .file
            .write_all(b"]\n")
            .and_then(|_| output.file.flush())
            .and_then(|_| fs::rename(&self.path, self.path.with_extension("json")))
            .map_err(|e| {
                eprintln!("Could not finalize game trace: {e}");
                OTelSdkError::InternalFailure(e.to_string())
            })
    }
}
