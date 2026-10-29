use time::OffsetDateTime;
use time::macros::format_description;
use tracing::Level;

/// Configuration for the tracing subscriber layer that writes to stderr.
/// Writes are synchronous and blocking. Only recommended for development and testing.
#[derive(Debug, Clone)]
pub enum StderrConfig {
    Disabled,
    Enabled {
        /// The maximum level to log. Overrides `RUST_LOG` environment variable for this layer.
        level: Level,
    },
}

use tracing::field::Field;
use tracing::field::Visit;

struct FieldCollector {
    fields: Vec<(String, String)>,
}

impl Visit for FieldCollector {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields
            .push((field.name().to_string(), format!("{:?}", value)));
    }
}

use tracing::Subscriber;
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::fmt::FormatEvent;
use tracing_subscriber::fmt::FormatFields;
use tracing_subscriber::fmt::format::Writer;

use crate::json_attributes::JsonAttributes;

pub struct StderrLogFormatter;

impl<S, N> FormatEvent<S, N> for StderrLogFormatter
where
    S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        // Metadata
        let meta = event.metadata();

        let ts_format = format_description!(
            "[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:6]"
        );

        let now_ts = OffsetDateTime::now_local().unwrap();
        let now_formatted = now_ts.format(&ts_format).unwrap();
        write!(writer, "{} ", now_formatted)?;

        write!(writer, "{:<5} ", meta.level())?;

        let file = meta.file().unwrap_or("<unknown_file>");
        let line = meta.line().unwrap_or(0);

        write!(writer, "{}:{}", file, line)?;

        writeln!(writer)?;

        // Message
        let mut visitor = FieldCollector { fields: Vec::new() };
        event.record(&mut visitor);

        if let Some(pos) = visitor.fields.iter().position(|(k, _)| k == "message") {
            let (_, msg) = visitor.fields.remove(pos);
            write!(writer, "{}", msg)?;
        } else {
            write!(writer, "Span: {}", meta.name())?;
        }

        writeln!(writer)?;

        // Attributes

        for (k, v) in visitor.fields {
            writeln!(writer, "  {} = {}", k, v)?;
        }

        if let Some(scope) = ctx.event_scope() {
            for span in scope {
                match span.extensions().get::<JsonAttributes>() {
                    Some(span_map) => {
                        for (k, v) in span_map {
                            writeln!(writer, "  {} = {}", k, v)?;
                        }
                    }
                    None => {
                        eprintln!(
                            "[StderrLogFormatter] Expected span extension JsonAttributes. Did you add [JsonAttributesCollectionLayer] to your tracing subscriber?"
                        );
                    }
                }
            }
        }

        Ok(())
    }
}
