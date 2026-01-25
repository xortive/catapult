//! Annotation caching for traffic monitor

use std::collections::{HashMap, VecDeque};

use cat_protocol::display::{decode_and_annotate_with_hint, AnnotatedFrame};
use cat_protocol::Protocol;

use super::TrafficMonitor;

/// Maximum number of entries in the annotation cache
pub(super) const ANNOTATION_CACHE_MAX_SIZE: usize = 1000;

/// Cache key for AnnotatedFrame results
///
/// Combines a hash of the raw bytes with the protocol hint to create
/// a unique key for caching decoded frames.
#[derive(Clone, Eq, PartialEq, Hash)]
pub(super) struct AnnotationCacheKey {
    /// Hash of the raw bytes (using FxHash-style computation for speed)
    bytes_hash: u64,
    /// Length of bytes (to distinguish different length inputs with same hash)
    bytes_len: usize,
    /// Protocol hint used for decoding
    protocol: Option<Protocol>,
}

impl AnnotationCacheKey {
    /// Create a new cache key from raw bytes and protocol hint
    pub(super) fn new(bytes: &[u8], protocol: Option<Protocol>) -> Self {
        // Fast hash computation (FxHash-style)
        let mut hash: u64 = 0;
        for &byte in bytes {
            hash = hash.rotate_left(5) ^ (byte as u64);
            hash = hash.wrapping_mul(0x517cc1b727220a95);
        }

        Self {
            bytes_hash: hash,
            bytes_len: bytes.len(),
            protocol,
        }
    }
}

/// Type alias for the annotation cache
pub(super) type AnnotationCache = HashMap<AnnotationCacheKey, Option<AnnotatedFrame>>;

/// Type alias for the cache order queue
pub(super) type CacheOrder = VecDeque<AnnotationCacheKey>;

impl TrafficMonitor {
    /// Get cached annotation or compute and cache it
    pub(super) fn get_cached_annotation(
        &mut self,
        data: &[u8],
        protocol: Option<Protocol>,
    ) -> Option<AnnotatedFrame> {
        let key = AnnotationCacheKey::new(data, protocol);

        // Check cache first
        if let Some(cached) = self.annotation_cache.get(&key) {
            return cached.clone();
        }

        // Decode and cache
        let result = decode_and_annotate_with_hint(data, protocol);

        // Evict oldest entry if cache is full
        if self.annotation_cache.len() >= ANNOTATION_CACHE_MAX_SIZE {
            if let Some(old_key) = self.cache_order.pop_front() {
                self.annotation_cache.remove(&old_key);
            }
        }

        // Insert into cache
        self.annotation_cache.insert(key.clone(), result.clone());
        self.cache_order.push_back(key);

        result
    }
}
