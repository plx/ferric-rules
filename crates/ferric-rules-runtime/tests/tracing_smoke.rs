#![cfg(feature = "tracing")]

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

use ferric_rules_runtime::{Engine, EngineConfig, HaltReason, RunLimit};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{Layer, Registry};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordKind {
    Span,
    Event,
}

#[derive(Debug, Eq, PartialEq)]
struct TraceRecord {
    kind: RecordKind,
    name: String,
    fields: BTreeMap<String, String>,
}

#[derive(Clone, Default)]
struct RecordingLayer {
    records: Arc<Mutex<Vec<TraceRecord>>>,
}

impl RecordingLayer {
    fn snapshot(&self) -> Vec<TraceRecord> {
        let records = self.records.lock().expect("trace recorder lock poisoned");
        records
            .iter()
            .map(|record| TraceRecord {
                kind: record.kind,
                name: record.name.clone(),
                fields: record.fields.clone(),
            })
            .collect()
    }

    fn push(&self, record: TraceRecord) {
        self.records
            .lock()
            .expect("trace recorder lock poisoned")
            .push(record);
    }
}

impl<S> Layer<S> for RecordingLayer
where
    S: Subscriber,
{
    fn on_new_span(
        &self,
        attributes: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _context: Context<'_, S>,
    ) {
        let mut visitor = FieldVisitor::default();
        attributes.record(&mut visitor);
        self.push(TraceRecord {
            kind: RecordKind::Span,
            name: attributes.metadata().name().to_owned(),
            fields: visitor.fields,
        });
    }

    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        self.push(TraceRecord {
            kind: RecordKind::Event,
            name: event.metadata().name().to_owned(),
            fields: visitor.fields,
        });
    }
}

#[derive(Default)]
struct FieldVisitor {
    fields: BTreeMap<String, String>,
}

impl Visit for FieldVisitor {
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_owned(), value.to_string());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name().to_owned(), value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name().to_owned(), value.to_string());
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name().to_owned(), value.to_owned());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.fields
            .insert(field.name().to_owned(), format!("{value:?}"));
    }
}

#[test]
fn trace_enabled_engine_run_reaches_a_subscriber_with_completion_fields() {
    let recorder = RecordingLayer::default();
    let subscriber = Registry::default().with(recorder.clone());

    let result = tracing::subscriber::with_default(subscriber, || {
        let mut engine = Engine::new(EngineConfig::utf8());
        engine
            .load_str(
                r"
                (deffacts startup (ready))
                (defrule fire-once
                    (ready)
                    =>
                    (assert (done)))
                ",
            )
            .expect("tracing smoke rules should load");
        engine.reset().expect("tracing smoke engine should reset");
        engine
            .run(RunLimit::Unlimited)
            .expect("tracing smoke engine should run")
    });

    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.halt_reason, HaltReason::AgendaEmpty);

    let records = recorder.snapshot();
    for expected_span in ["engine_load_str", "engine_reset", "engine_run"] {
        assert!(
            records
                .iter()
                .any(|record| { record.kind == RecordKind::Span && record.name == expected_span }),
            "trace subscriber did not observe {expected_span}; records: {records:#?}"
        );
    }

    let completion = records
        .iter()
        .find(|record| {
            record.kind == RecordKind::Event
                && record.fields.get("message").map(String::as_str) == Some("engine_run_complete")
        })
        .unwrap_or_else(|| {
            panic!("trace subscriber did not observe engine_run_complete: {records:#?}")
        });
    assert_eq!(
        completion.fields.get("rules_fired").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        completion.fields.get("halt_reason").map(String::as_str),
        Some("agenda_empty")
    );
}
