//! Traffic monitor UI component
//!
//! This module provides a traffic monitoring UI that displays CAT protocol
//! traffic between radios and amplifiers, with support for filtering,
//! export, and diagnostic messages.

use std::collections::VecDeque;

use tracing::Level;

mod cache;
mod export;
mod ingest;
mod models;
mod ui;

// Re-export public types (used by TrafficEntry fields and for pattern matching)
#[allow(unused_imports)]
pub use models::{DiagnosticSeverity, ExportAction, TrafficDirection, TrafficEntry, TrafficSource};

use cache::{AnnotationCache, CacheOrder, ANNOTATION_CACHE_MAX_SIZE};
use models::TrafficDirection as Direction;

/// Traffic monitor state
pub struct TrafficMonitor {
    /// Traffic entries
    entries: VecDeque<TrafficEntry>,
    /// Maximum entries to keep
    max_entries: usize,
    /// Auto-scroll to bottom
    auto_scroll: bool,
    /// Filter by direction
    filter_direction: Option<Direction>,
    /// Show simulated traffic
    show_simulated: bool,
    /// Pause monitoring
    paused: bool,
    /// Minimum diagnostic level to show (None = off, Some(Level::DEBUG) = all)
    /// Events at this level and above are shown (filtering happens at tracing layer)
    diagnostic_level: Option<Level>,
    /// Cache for AnnotatedFrame results to avoid redundant parsing
    annotation_cache: AnnotationCache,
    /// Keys in insertion order for LRU-style eviction
    cache_order: CacheOrder,
}

impl TrafficMonitor {
    /// Create a new traffic monitor
    ///
    /// `diagnostic_level` controls which diagnostic events are shown:
    /// - `None` = off (no diagnostics shown)
    /// - `Some(Level::ERROR)` = only errors
    /// - `Some(Level::WARN)` = warnings and errors
    /// - `Some(Level::INFO)` = info, warnings, and errors
    /// - `Some(Level::DEBUG)` = all diagnostics
    pub fn new(max_entries: usize, diagnostic_level: Option<Level>) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_entries),
            max_entries,
            auto_scroll: true,
            filter_direction: None,
            show_simulated: true,
            paused: false,
            diagnostic_level,
            annotation_cache: AnnotationCache::with_capacity(ANNOTATION_CACHE_MAX_SIZE),
            cache_order: CacheOrder::with_capacity(ANNOTATION_CACHE_MAX_SIZE),
        }
    }

    /// Get the current diagnostic level
    pub fn diagnostic_level(&self) -> Option<Level> {
        self.diagnostic_level
    }

    /// Clear all entries and the annotation cache
    pub fn clear(&mut self) {
        self.entries.clear();
        self.annotation_cache.clear();
        self.cache_order.clear();
    }
}
