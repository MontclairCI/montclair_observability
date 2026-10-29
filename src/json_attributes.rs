use serde_json::Value;
use tracing::Subscriber;
use tracing::field::Field;
use tracing::field::Visit;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

pub type JsonAttributes = serde_json::Map<String, Value>;

pub struct JsonAttributesVisitor<'a> {
    prefix: Option<&'a str>,
    map: &'a mut JsonAttributes,
}

impl<'a> JsonAttributesVisitor<'a> {
    pub fn new(prefix: Option<&'a str>, map: &'a mut JsonAttributes) -> Self {
        Self { prefix, map }
    }
}

impl Visit for JsonAttributesVisitor<'_> {
    fn record_i64(&mut self, field: &Field, value: i64) {
        match self.prefix {
            Some(prefix) => {
                self.map
                    .insert(format!("{}.{}", prefix, field.name()), Value::from(value));
            }
            None => {
                self.map
                    .insert(field.name().to_string(), Value::from(value));
            }
        }
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        match self.prefix {
            Some(prefix) => {
                self.map
                    .insert(format!("{}.{}", prefix, field.name()), Value::from(value));
            }
            None => {
                self.map
                    .insert(field.name().to_string(), Value::from(value));
            }
        }
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        match self.prefix {
            Some(prefix) => {
                self.map
                    .insert(format!("{}.{}", prefix, field.name()), Value::from(value));
            }
            None => {
                self.map
                    .insert(field.name().to_string(), Value::from(value));
            }
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        match self.prefix {
            Some(prefix) => {
                self.map
                    .insert(format!("{}.{}", prefix, field.name()), Value::from(value));
            }
            None => {
                self.map
                    .insert(field.name().to_string(), Value::from(value));
            }
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        match self.prefix {
            Some(prefix) => {
                self.map.insert(
                    format!("{}.{}", prefix, field.name()),
                    Value::from(format!("{:?}", value)),
                );
            }
            None => {
                self.map.insert(
                    field.name().to_string(),
                    Value::from(format!("{:?}", value)),
                );
            }
        }
    }
}

/// A layer that collects span fields in a JSON map and stores them in
/// extensions so they are available to other layers.
pub struct JsonAttributesCollectionLayer;

impl<S> Layer<S> for JsonAttributesCollectionLayer
where
    S: Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        let map = {
            let prefix = attrs.metadata().name();
            let mut map = JsonAttributes::new();
            let mut visitor = JsonAttributesVisitor::new(Some(prefix), &mut map);
            attrs.record(&mut visitor);
            map
        };

        let span = ctx.span(id).expect("Span must exist");
        let mut extensions = span.extensions_mut();
        extensions.insert(map);
    }

    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        ctx: Context<'_, S>,
    ) {
        let span = ctx.span(id).expect("Span must exist");
        let mut extensions = span.extensions_mut();
        if let Some(map) = extensions.get_mut::<JsonAttributes>() {
            let prefix = span.metadata().name();
            let mut visitor = JsonAttributesVisitor::new(Some(prefix), map);
            values.record(&mut visitor);
        }
    }
}
