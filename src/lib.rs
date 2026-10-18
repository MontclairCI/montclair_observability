use std::sync::Once;
use tracing_error::SpanTrace;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub use tracing::error;
pub use tracing::info;
pub use tracing::warn;

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

impl std::fmt::Display for TError {
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

/// Guards single installation of the tracing subscriber.
static SUBSCRIBER_INSTALLED: Once = Once::new();

/// Guards single installation of the process-wide panic hook.
static PANIC_HOOK_INSTALLED: Once = Once::new();

#[derive(thiserror::Error, Debug)]
pub enum LoggingError {
    #[error("Failed to create log directory: {0}")]
    CreateLogDirectoryFailed(#[source] std::io::Error),
}

/// Initialize tracing for debug builds.
#[cfg(all(debug_assertions, not(test)))]
#[tracing::instrument(skip_all)]
pub fn init_tracing() -> Result<Option<tracing_appender::non_blocking::WorkerGuard>, TError> {
    SUBSCRIBER_INSTALLED.call_once(|| {
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::from_default_env())
            .with(tracing_subscriber::fmt::Layer::new().with_writer(std::io::stderr))
            .with(tracing_error::ErrorLayer::default()) // Required for capturing SpanTrace
            .init();
    });

    install_panic_hook();
    Ok(None)
}

/// Initialize tracing for release/test/bench builds.
#[cfg(any(not(debug_assertions), test))]
#[tracing::instrument(skip_all)]
pub fn init_tracing() -> Result<Option<tracing_appender::non_blocking::WorkerGuard>, TError> {
    let log_directory = ".".to_string();
    std::fs::create_dir_all(&log_directory).map_err(LoggingError::CreateLogDirectoryFailed)?;

    let (file_writer, guard) = tracing_appender::non_blocking(tracing_appender::rolling::never(
        &log_directory,
        "muzanci-server.log",
    ));

    let mut installed = false;
    SUBSCRIBER_INSTALLED.call_once(|| {
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::Layer::new()
                    .json()
                    .flatten_event(true)
                    .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
                    .with_writer(file_writer.clone()),
            )
            .with(tracing_error::ErrorLayer::default()) // Required for capturing SpanTrace
            .init();
        installed = true;
    });

    install_panic_hook();
    Ok(if installed { Some(guard) } else { None })
}

/// Install a process-wide panic hook (idempotent).
///
/// Logs the panic via `tracing::error!` and cancels the registered
/// cancellation token, then chains to the previous hook.
fn install_panic_hook() {
    PANIC_HOOK_INSTALLED.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let payload = panic_payload(info);
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "<unknown>".to_string());

            let span_trace = tracing_error::SpanTrace::capture();

            tracing::error!(
                panic.payload = %payload,
                panic.location = %location,
                panic.span_trace = %span_trace,
            );

            prev(info);
        }));
    });
}

/// Extract the panic payload as a string when it is a `&str` or `String`.
fn panic_payload(info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Calling `init` twice must not panic; the hook is installed once.
    #[test]
    fn init_is_idempotent() {
        let _ = init_tracing().unwrap();
        let _ = init_tracing().unwrap();
    }
}
