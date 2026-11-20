use clickhouse::Client;
use clickhouse::Row;
use serde::Serialize;
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio_util::sync::CancellationToken;
use tracing::Level;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use url::Url;

use crate::TError;
use crate::json_attributes::JsonAttributes;
use crate::json_attributes::JsonAttributesVisitor;

/// Configuration for the tracing subscriber layer that writes to Clickhouse.
#[derive(Debug, Clone)]
pub enum ClickhouseConfig {
    Disabled,
    Enabled {
        /// The maximum level to log. Overrides `RUST_LOG` environment variable for this layer.
        level: Level,
        username: String,
        password: String,
        database: String,
        url: Url,
        shutdown: CancellationToken,
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
    pub async fn new(
        username: &str,
        password: &str,
        database: &str,
        url: &Url,
        shutdown: CancellationToken,
    ) -> Result<Self, TError> {
        let client_handle =
            ClickhouseClient::spawn(username, password, database, url, shutdown).await?;
        Ok(Self { client_handle })
    }
}

impl<S> Layer<S> for ClickhouseLayer
where
    S: Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let mut map = serde_json::Map::new();

        if let Some(scope) = ctx.event_scope(event) {
            for span in scope {
                match span.extensions().get::<JsonAttributes>() {
                    Some(span_map) => {
                        for (k, v) in span_map {
                            map.insert(k.clone(), v.clone());
                        }
                    }
                    None => {
                        eprintln!(
                            "[ClickhouseLayer] Expected span extension JsonAttributes. Did you add [JsonAttributesCollectionLayer] to your tracing subscriber?"
                        );
                    }
                }
            }
        }

        let mut visitor = JsonAttributesVisitor::new(None, &mut map);
        event.record(&mut visitor);

        let level = event.metadata().level().to_string();

        let target = event.metadata().target().to_string();

        let message = map
            .remove("message")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();

        let error_span_trace = map
            .remove("error.span_trace")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();

        let attr = match serde_json::to_string(&map) {
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

        self.client_handle.try_send(row);
    }
}

/// A handle to the Clickhouse client actor task.
#[derive(Clone)]
struct ClickhouseClientHandle {
    tx: mpsc::Sender<WideEventRow>,
}

impl ClickhouseClientHandle {
    fn try_send(&self, row: WideEventRow) {
        match self.tx.try_send(row) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                eprintln!("[ClickhouseClientHandle::try_send] Failed to send: channel full");
            }
            Err(TrySendError::Closed(_)) => {
                eprintln!("[ClickhouseClientHandle::try_send] Failed to send: channel closed");
            }
        }
    }
}

#[derive(Row, Serialize, Clone)]
pub struct WideEventRow {
    /// The timestamp of the event.
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    pub timestamp: OffsetDateTime,

    /// The log level of the event.
    pub level: String,

    /// The name of the crate and module that generated the event.
    pub target: String,

    /// A human-readable message describing the event.
    pub message: String,

    /// The span trace collected by [`TError`].
    /// An empty string if the event is not a [`TError`].
    pub error_span_trace: String,

    /// A JSON string of the event attributes.
    /// The crate `clickhouse@0.15.3` only supports inserting a JSON column from a Rust String.
    /// https://github.com/Clickhouse/clickhouse-rs/blob/dad4c6dd8a40aaeaa7534abf38155219f6fd73b5/examples/data_types_new_json.rs#L73
    pub attr: String,
}

#[derive(Debug, thiserror::Error)]
#[error("ClickhouseClientError: {0}")]
struct ClickhouseClientError(String);

/// An actor that spawns a task to receive events from other tasks, batch events, and send to Clickhouse.
struct ClickhouseClient {
    client: clickhouse::Client,
    rx: mpsc::Receiver<WideEventRow>,
}

impl ClickhouseClient {
    async fn spawn(
        username: &str,
        password: &str,
        database: &str,
        url: &Url,
        shutdown: CancellationToken,
    ) -> Result<ClickhouseClientHandle, TError> {
        let (tx, rx) = mpsc::channel(512);

        let client = Client::default()
            .with_user(username)
            .with_password(password)
            .with_database(database)
            .with_url(url.as_str())
            // Enable inserting JSON columns as a string
            .with_setting("input_format_binary_read_json_as_string", "1")
            // Enable selecting JSON columns as a string
            .with_setting("output_format_binary_write_json_as_string", "1");

        // TODO: Insert a test row into the "event" table to validate table and column existence.
        client
            .query("SELECT 1")
            .fetch_one::<u8>()
            .await
            .map_err(|e| {
                ClickhouseClientError(format!(
                    "Clickhouse client configuration check failed: {e:?}"
                ))
            })?;

        tokio::spawn(async move { ClickhouseClient { client, rx }.main(shutdown).await });

        Ok(ClickhouseClientHandle { tx })
    }

    async fn main(mut self, shutdown: CancellationToken) {
        if let Err(e) = self.run().await {
            eprintln!("ClickhouseClient: error: {}", e);
            shutdown.cancel();
        }
    }

    async fn run(&mut self) -> Result<(), TError> {
        let mut batch = Vec::with_capacity(256);
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));

        loop {
            tokio::select! {
                Some(row) = self.rx.recv() => {
                    batch.push(row);
                    if batch.len() >= 256 {
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
        let mut inserter = match self.client.insert::<WideEventRow>("event").await {
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
