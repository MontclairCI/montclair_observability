use clickhouse::Client;
use clickhouse::Row;
use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tracing::Level;
use tracing::Subscriber;
use tracing::field::Field;
use tracing::field::Visit;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

use crate::TError;

/// Configuration for the tracing subscriber layer that writes to Clickhouse.
#[derive(Debug, Clone)]
pub enum ClickhouseConfig {
    Disabled,
    Enabled {
        /// The maximum level to log. Overrides `RUST_LOG` environment variable for this layer.
        level: Level,
        server_url: String,
        database: String,
        table: String,
    },
}

/// A [`Layer`] that sends tracing events to a Clickhouse database.
///
/// Events are sent asynchronously and requires a [`tokio::runtime::Handle`] to spawn the actor task.
/// If the actor's channel gets full, then events will be blocked.
pub struct ClickhouseLayer {
    client_handle: ClickhouseClientHandle,
}

impl ClickhouseLayer {
    pub fn new(server_url: String, database: String, table: String) -> Self {
        let client_handle = ClickhouseClient::spawn(server_url, database, table);
        Self { client_handle }
    }
}

impl<S> Layer<S> for ClickhouseLayer
where
    S: Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let mut attr_map = serde_json::Map::new();

        if let Some(scope) = ctx.event_scope(event) {
            for span in scope {
                let extensions = span.extensions();
                if let Some(cached_fields) = extensions.get::<serde_json::Map<String, Value>>() {
                    for (k, v) in cached_fields {
                        attr_map.insert(k.clone(), v.clone());
                    }
                }
            }
        }

        let mut visitor = EventVisitor { map: &mut attr_map };
        event.record(&mut visitor);

        let level = event.metadata().level().to_string();

        let target = event.metadata().target().to_string();

        let message = attr_map
            .remove("message")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();

        let error_span_trace = attr_map
            .remove("error.span_trace")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();

        let attr = match serde_json::to_string(&attr_map) {
            Ok(attr) => attr,
            Err(_) => {
                eprintln!(
                    "[ClickhouseLayer] Failed to serialize event attributes. Dropping attributes from event."
                );
                "{}".to_string()
            }
        };

        let row = WideEventRow {
            timestamp: OffsetDateTime::now_utc(),
            level,
            target,
            message,
            error_span_trace,
            attr,
        };

        self.client_handle.blocking_send(row);
    }

    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        let span = ctx.span(id).expect("Span must exist");
        let mut map = serde_json::Map::new();
        let mut visitor = EventVisitor { map: &mut map };
        attrs.record(&mut visitor);
        let mut extensions = span.extensions_mut();
        extensions.insert(map);
    }
}

struct EventVisitor<'a> {
    map: &'a mut serde_json::Map<String, Value>,
}

impl Visit for EventVisitor<'_> {
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.map
            .insert(field.name().to_string(), Value::from(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.map
            .insert(field.name().to_string(), Value::from(value));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.map
            .insert(field.name().to_string(), Value::from(value));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.map
            .insert(field.name().to_string(), Value::from(value));
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.map.insert(
            field.name().to_string(),
            Value::from(format!("{:?}", value)),
        );
    }
}

/// A handle to the Clickhouse client actor task.
#[derive(Clone)]
struct ClickhouseClientHandle {
    tx: mpsc::Sender<WideEventRow>,
}

impl ClickhouseClientHandle {
    fn blocking_send(&self, row: WideEventRow) {
        if self.tx.blocking_send(row).is_err() {
            eprintln!(
                "[ClickhouseClientHandle::blocking_send] Failed to send wide event row: channel receiver dropped"
            );
        }
    }
}

#[derive(Row, Serialize, Clone)]
pub struct WideEventRow {
    /// The timestamp of the event.
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    timestamp: OffsetDateTime,

    /// The log level of the event.
    level: String,

    /// The name of the crate and module that generated the event.
    target: String,

    /// A human-readable message describing the event.
    message: String,

    /// The span trace collected by [`TError`].
    /// An empty string if the event is not a [`TError`].
    error_span_trace: String,

    /// A JSON string of the event attributes.
    /// The crate `clickhouse@0.15.3` only supports inserting a JSON column from a Rust String.
    /// https://github.com/Clickhouse/clickhouse-rs/blob/dad4c6dd8a40aaeaa7534abf38155219f6fd73b5/examples/data_types_new_json.rs#L73
    attr: String,
}

#[derive(Debug, thiserror::Error)]
#[error("ClickhouseClientError: {0}")]
struct ClickhouseClientError(String);

/// An actor that spawns a task to receive events from other tasks, batch events, and send to Clickhouse.
struct ClickhouseClient {
    client: clickhouse::Client,
    table: String,
    rx: mpsc::Receiver<WideEventRow>,
}

impl ClickhouseClient {
    fn spawn(server_url: String, database: String, table: String) -> ClickhouseClientHandle {
        let (tx, rx) = mpsc::channel(1024);

        let client = Client::default()
            .with_url(server_url)
            .with_database(database);

        tokio::spawn(async move { ClickhouseClient { client, table, rx }.main().await });

        ClickhouseClientHandle { tx }
    }

    async fn main(mut self) -> Result<(), TError> {
        let mut batch = Vec::with_capacity(500);
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));

        loop {
            tokio::select! {
                Some(row) = self.rx.recv() => {
                    batch.push(row);
                    if batch.len() >= 500 {
                        self.flush_batch(&batch).await?;
                        batch.clear();
                    }
                }

                _ = interval.tick() => {
                    if !batch.is_empty() {
                        self.flush_batch(&batch).await?;
                        batch.clear();
                    }
                }
            }
        }
    }

    async fn flush_batch(&mut self, batch: &[WideEventRow]) -> Result<(), ClickhouseClientError> {
        let mut inserter = match self.client.insert::<WideEventRow>(&self.table).await {
            Ok(inserter) => inserter,
            Err(e) => {
                let e = ClickhouseClientError(format!(
                    "[ClickhouseClient] Failed to initialize inserter: {e}"
                ));
                return Err(e);
            }
        };

        for row in batch {
            if let Err(e) = inserter.write(row).await {
                let e = ClickhouseClientError(format!(
                    "[ClickhouseClient] Failed to write row into batch buffer: {e}"
                ));
                return Err(e);
            }
        }

        if let Err(e) = inserter.end().await {
            let e = ClickhouseClientError(format!(
                "[ClickhouseClient] Failed to commit batch insert: {e}"
            ));
            return Err(e);
        }

        Ok(())
    }
}
