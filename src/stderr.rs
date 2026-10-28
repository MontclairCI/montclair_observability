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
