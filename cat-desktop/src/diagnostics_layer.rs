//! Custom tracing layer for sending log events to the diagnostics portal

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use tracing::field::{Field, Visit};
use tracing::subscriber::Interest;
use tracing::{Event, Level, Metadata, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// Crates that belong to this project (for filtering)
const PROJECT_CRATES: &[&str] = &[
    "catapult",
    "cat_protocol",
    "cat_detect",
    "cat_mux",
    "cat_sim",
];

/// Shared state for dynamic level filtering
///
/// Uses atomic operations for lock-free level changes.
/// Level encoding: 0=off, 1=error, 2=warn, 3=info, 4=debug, 5=trace
pub struct DiagnosticLevelState {
    level: AtomicU8,
}

impl DiagnosticLevelState {
    /// Create new state with the given initial level
    pub fn new(level: Option<Level>) -> Self {
        Self {
            level: AtomicU8::new(Self::level_to_u8(level)),
        }
    }

    /// Update the filter level (atomic store)
    pub fn set_level(&self, level: Option<Level>) {
        self.level
            .store(Self::level_to_u8(level), Ordering::Relaxed);
    }

    /// Get the current filter level (atomic load)
    pub fn get_level(&self) -> Option<Level> {
        Self::u8_to_level(self.level.load(Ordering::Relaxed))
    }

    fn level_to_u8(level: Option<Level>) -> u8 {
        match level {
            None => 0,
            Some(Level::ERROR) => 1,
            Some(Level::WARN) => 2,
            Some(Level::INFO) => 3,
            Some(Level::DEBUG) => 4,
            Some(Level::TRACE) => 5,
        }
    }

    fn u8_to_level(value: u8) -> Option<Level> {
        match value {
            0 => None,
            1 => Some(Level::ERROR),
            2 => Some(Level::WARN),
            3 => Some(Level::INFO),
            4 => Some(Level::DEBUG),
            _ => Some(Level::TRACE),
        }
    }
}

/// Filter that checks project crate membership and dynamic level via atomic state
pub struct ProjectCrateFilter {
    state: Arc<DiagnosticLevelState>,
}

impl ProjectCrateFilter {
    /// Create a new filter with shared state
    pub fn new(state: Arc<DiagnosticLevelState>) -> Self {
        Self { state }
    }
}

impl<S> tracing_subscriber::layer::Filter<S> for ProjectCrateFilter {
    fn enabled(&self, meta: &Metadata<'_>, _cx: &Context<'_, S>) -> bool {
        // Check if this is from a project crate
        let target = meta.target();
        let is_project_crate = PROJECT_CRATES
            .iter()
            .any(|crate_name| target.starts_with(crate_name));

        if !is_project_crate {
            return false;
        }

        // Compare event level against current filter level
        match self.state.get_level() {
            None => false, // Filter is off
            Some(filter_level) => *meta.level() <= filter_level,
        }
    }

    fn callsite_enabled(&self, meta: &'static Metadata<'static>) -> Interest {
        // Check project crate membership (static, won't change)
        let target = meta.target();
        let is_project_crate = PROJECT_CRATES
            .iter()
            .any(|crate_name| target.starts_with(crate_name));

        if !is_project_crate {
            // Non-project crates are never enabled
            Interest::never()
        } else {
            // Project crates need per-event checks since level is dynamic
            Interest::sometimes()
        }
    }
}

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

        // Filtering is handled by the EnvFilter attached to this layer.
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
}
