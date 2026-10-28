use serde_json::Value;
use tracing::Subscriber;
use tracing::field::Field;
use tracing::field::Visit;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

pub type JsonAttributes = serde_json::Map<String, Value>;

pub struct JsonAttributesVisitor<'a> {
    map: &'a mut JsonAttributes,
}

impl<'a> JsonAttributesVisitor<'a> {
    pub fn new(map: &'a mut JsonAttributes) -> Self {
        Self { map }
    }
}

impl Visit for JsonAttributesVisitor<'_> {
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
            let mut map = JsonAttributes::new();
            let mut visitor = JsonAttributesVisitor::new(&mut map);
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
        if let Some(mut map) = extensions.get_mut::<JsonAttributes>() {
            let mut visitor = JsonAttributesVisitor::new(&mut map);
            values.record(&mut visitor);
        }
    }
}
