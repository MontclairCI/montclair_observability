use std::fmt::Display;
use std::sync::Once;
use tracing::Level;
use tracing_error::SpanTrace;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub use tracing::error;
pub use tracing::info;
pub use tracing::warn;

use crate::clickhouse::ClickhouseConfig;
use crate::clickhouse::ClickhouseLayer;
use crate::file::FileConfig;
use crate::file::FileWriter;
use crate::json_attributes::JsonAttributesCollectionLayer;
use crate::stderr::StderrConfig;
use crate::stderr::StderrLogFormatter;

mod clickhouse;
mod file;
mod json_attributes;
mod stderr;

/// A tracing error with logical span hierarchy.
#[derive(Debug)]
pub struct TError {
    type_name: &'static str,
    source: anyhow::Error,
    span_trace: SpanTrace,
}

impl TError {
    pub fn new(s: &str) -> Self {
        Self {
            type_name: "&str",
            source: anyhow::Error::msg(s.to_string()),
            span_trace: SpanTrace::capture(),
        }
    }

    pub fn source(self) -> anyhow::Error {
        self.source
    }
}

impl Display for TError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {}\nSpan Trace:\n{}",
            self.type_name, self.source, self.span_trace
        )
    }
}

impl<E> From<E> for TError
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn from(err: E) -> Self {
        Self {
            type_name: std::any::type_name::<E>(),
            source: anyhow::Error::new(err),
            span_trace: SpanTrace::capture(),
        }
    }
}

/// Configuration for the tracing subscriber layers.
#[derive(Debug, Clone)]
pub struct TracingConfig {
    pub stderr: StderrConfig,
    pub file: FileConfig,
    pub clickhouse: ClickhouseConfig,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            stderr: StderrConfig::Enabled { level: Level::INFO },
            file: FileConfig::Disabled,
            clickhouse: ClickhouseConfig::Disabled,
        }
    }
}

/// Guards single installation of the process-wide tracing subscriber.
static TRACING_INSTALLED: Once = Once::new();

/// Initializes the tracing subscriber with the given configuration.
/// Returns `Some(WorkerGuard)` if FileConfig is Enabled.
/// The `WorkerGuard` must be kept alive for the duration of the process to ensure all logs are flushed.
pub fn init_tracing(config: TracingConfig) -> Result<(), TError> {
    let registry = tracing_subscriber::registry().with(tracing_error::ErrorLayer::default()); // Required for SpanTrace

    let stderr_layer = match config.stderr {
        StderrConfig::Disabled => None,
        StderrConfig::Enabled { level } => Some(
            tracing_subscriber::fmt::Layer::new()
                .event_format(StderrLogFormatter::new())
                .with_writer(std::io::stderr)
                .with_filter(EnvFilter::from_default_env().add_directive(level.into()))
                .boxed(),
        ),
    };

    let file_layer = match config.file {
        FileConfig::Enabled {
            level,
            dir_path,
            file_name,
        } => {
            let file_writer = FileWriter::spawn(dir_path, file_name);

            Some(
                tracing_subscriber::fmt::Layer::new()
                    .json()
                    .flatten_event(true)
                    .with_writer(file_writer)
                    .with_filter(EnvFilter::from_default_env().add_directive(level.into()))
                    .boxed(),
            )
        }
        FileConfig::Disabled => None,
    };

    let clickhouse_layer = match config.clickhouse {
        ClickhouseConfig::Enabled {
            level,
            server_url,
            database,
            table,
        } => Some(
            ClickhouseLayer::new(server_url, database, table)
                .with_filter(
                    tracing_subscriber::EnvFilter::from_default_env().add_directive(level.into()),
                )
                .boxed(),
        ),
        ClickhouseConfig::Disabled => None,
    };

    let mut installed = false;
    TRACING_INSTALLED.call_once(|| {
        let subscriber = registry
            .with(JsonAttributesCollectionLayer)
            .with(stderr_layer)
            .with(file_layer)
            .with(clickhouse_layer);

        if subscriber.try_init().is_ok() {
            installed = true;
        }
    });

    install_panic_hook();

    Ok(())
}

/// Guards single installation of the process-wide panic hook.
static PANIC_HOOK_INSTALLED: Once = Once::new();

/// Install a process-wide panic hook (idempotent).
///
/// Logs the panic via `tracing::error!` then calls the existing hook.
fn install_panic_hook() {
    PANIC_HOOK_INSTALLED.call_once(|| {
        let next = std::panic::take_hook();

        std::panic::set_hook(Box::new(move |info| {
            let payload = info
                .payload_as_str()
                .unwrap_or("<non-string panic payload>")
                .to_string();

            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "<unknown>".to_string());

            tracing::error!(
                panic.payload = %payload,
                panic.location = %location,
            );

            next(info);
        }));
    });
}
