use std::collections::BTreeMap;
use std::fmt;
use std::fs::OpenOptions;
use std::io;
use std::sync::Mutex;

use serde_json::Value;
use tracing::field::{Field, Visit};
use tracing::span::Attributes;
use tracing::{Event, Id, Subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::Context;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;

pub fn init() -> anyhow::Result<()> {
    let log_path = std::env::var("LOG_FILE_PATH").unwrap_or_else(|_| "logs/ledger-api.log".into());
    if let Some(parent) = std::path::Path::new(&log_path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let file = OpenOptions::new().create(true).append(true).open(&log_path)?;
    let file_writer = Mutex::new(file);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(CaptureSpanFieldsLayer)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(io::stdout)
                .event_format(JsonFormatter),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(file_writer)
                .event_format(JsonFormatter),
        )
        .init();

    Ok(())
}

#[derive(Default)]
struct SpanFields(BTreeMap<&'static str, Value>);

struct CaptureSpanFieldsLayer;

impl<S> Layer<S> for CaptureSpanFieldsLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut fields = SpanFields::default();
        attrs.record(&mut FieldVisitor(&mut fields.0));
        span.extensions_mut().insert(fields);
    }
}

struct FieldVisitor<'a>(&'a mut BTreeMap<&'static str, Value>);

impl Visit for FieldVisitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name(), Value::String(value.to_string()));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.insert(field.name(), Value::Bool(value));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0.insert(field.name(), Value::from(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name(), Value::from(value));
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.0.insert(field.name(), Value::from(value));
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.insert(field.name(), Value::String(format!("{value:?}")));
    }
}

struct JsonFormatter;

impl<S, N> FormatEvent<S, N> for JsonFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let metadata = event.metadata();
        let mut entry: serde_json::Map<String, Value> = serde_json::Map::new();

        entry.insert(
            "timestamp".into(),
            Value::String(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        );
        entry.insert("level".into(), Value::String(metadata.level().to_string()));
        entry.insert("target".into(), Value::String(metadata.target().to_string()));

        let mut event_fields: BTreeMap<&'static str, Value> = BTreeMap::new();
        event.record(&mut FieldVisitor(&mut event_fields));

        let message = event_fields
            .remove("message")
            .unwrap_or(Value::String(String::new()));
        entry.insert("message".into(), message);

        let mut request_id: Option<Value> = None;
        if let Some(scope) = ctx.event_scope() {
            for span in scope.from_root() {
                let exts = span.extensions();
                if let Some(span_fields) = exts.get::<SpanFields>() {
                    for (key, value) in &span_fields.0 {
                        if *key == "request_id" {
                            request_id = Some(value.clone());
                        } else if !entry.contains_key(*key) {
                            entry.insert((*key).to_string(), value.clone());
                        }
                    }
                }
            }
        }
        entry.insert(
            "request_id".into(),
            request_id.unwrap_or(Value::Null),
        );

        for (key, value) in event_fields {
            entry.insert(key.to_string(), value);
        }

        let serialized = serde_json::to_string(&Value::Object(entry))
            .map_err(|_| fmt::Error)?;
        writeln!(writer, "{serialized}")
    }
}
