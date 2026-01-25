//! Custom tracing layer for sending log events to the diagnostics portal

use std::sync::mpsc::Sender;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::Context;
use tracing_subscriber::reload;
use tracing_subscriber::Layer;

/// Type alias for the diagnostics filter reload handle
pub type DiagnosticsFilterHandle = reload::Handle<EnvFilter, tracing_subscriber::Registry>;

/// Build an EnvFilter for project crates at the specified level
///
/// When level is Some, events at that level and above are captured.
/// When level is None, no events are captured ("off").
pub fn build_diagnostics_filter(level: Option<Level>) -> EnvFilter {
    let level_str = match level {
        Some(Level::DEBUG) | Some(Level::TRACE) => "debug",
        Some(Level::INFO) => "info",
        Some(Level::WARN) => "warn",
        Some(Level::ERROR) => "error",
        None => "off",
    };
    // Build filter for project crates at specified level
    EnvFilter::new(format!(
        "catapult={l},cat_protocol={l},cat_detect={l},cat_mux={l},cat_sim={l}",
        l = level_str
    ))
}

/// Crate prefixes to capture in the diagnostics layer
const CRATE_PREFIXES: &[&str] = &[
    "catapult",
    "cat_protocol",
    "cat_detect",
    "cat_mux",
    "cat_sim",
];

/// A diagnostic event captured from tracing
#[derive(Debug, Clone)]
pub struct DiagnosticEvent {
    /// Source of the event (derived from tracing target or custom field)
    pub source: String,
    /// Severity level
    pub level: Level,
    /// Log message
    pub message: String,
}

/// Custom tracing layer that captures log events and sends them via channel
pub struct DiagnosticsLayer {
    tx: Sender<DiagnosticEvent>,
}

impl DiagnosticsLayer {
    /// Create a new DiagnosticsLayer that sends events to the given channel
    pub fn new(tx: Sender<DiagnosticEvent>) -> Self {
        Self { tx }
    }
}

impl<S: Subscriber> Layer<S> for DiagnosticsLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let target = event.metadata().target();

        // Only capture events from our crates
        if !CRATE_PREFIXES
            .iter()
            .any(|prefix| target.starts_with(prefix))
        {
            return;
        }

        // Extract message and optional source override from the event
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        // Use custom source if provided, otherwise derive from target
        let source = visitor.source.unwrap_or_else(|| simplify_target(target));

        let diagnostic = DiagnosticEvent {
            source,
            level: *event.metadata().level(),
            message: visitor.message.unwrap_or_default(),
        };

        // Send to channel (ignore errors if receiver is dropped)
        let _ = self.tx.send(diagnostic);
    }
}

/// Visitor to extract message and optional source from tracing fields
#[derive(Default)]
struct MessageVisitor {
    message: Option<String>,
    source: Option<String>,
}

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => self.message = Some(value.to_string()),
            "source" => self.source = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "message" => self.message = Some(format!("{:?}", value)),
            "source" => self.source = Some(format!("{:?}", value)),
            _ => {}
        }
    }
}

/// Simplify a module path target to a user-friendly source name
fn simplify_target(target: &str) -> String {
    // Get the last segment of the module path
    // e.g., "catapult::app" -> "App", "cat_protocol::icom" -> "Icom"
    target
        .rsplit("::")
        .next()
        .map(|s| {
            // Capitalize first letter
            let mut chars = s.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => s.to_string(),
            }
        })
        .unwrap_or_else(|| target.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simplify_target() {
        assert_eq!(simplify_target("catapult::app"), "App");
        assert_eq!(simplify_target("cat_protocol::icom"), "Icom");
        assert_eq!(simplify_target("simple"), "Simple");
        assert_eq!(simplify_target("cat_mux::state"), "State");
    }

    #[test]
    fn test_crate_filter() {
        // Our crates should match
        assert!(CRATE_PREFIXES
            .iter()
            .any(|p| "catapult::app".starts_with(p)));
        assert!(CRATE_PREFIXES
            .iter()
            .any(|p| "cat_protocol::icom".starts_with(p)));
        assert!(CRATE_PREFIXES
            .iter()
            .any(|p| "cat_detect::scanner".starts_with(p)));
        assert!(CRATE_PREFIXES
            .iter()
            .any(|p| "cat_mux::state".starts_with(p)));
        assert!(CRATE_PREFIXES
            .iter()
            .any(|p| "cat_sim::radio".starts_with(p)));

        // Third-party crates should not match
        assert!(!CRATE_PREFIXES
            .iter()
            .any(|p| "tokio::runtime".starts_with(p)));
        assert!(!CRATE_PREFIXES
            .iter()
            .any(|p| "egui::widgets".starts_with(p)));
        assert!(!CRATE_PREFIXES.iter().any(|p| "serialport".starts_with(p)));
    }
}
