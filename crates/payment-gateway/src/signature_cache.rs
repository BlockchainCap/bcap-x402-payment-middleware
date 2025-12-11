use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Cache for tracking used signatures to prevent replay attacks
pub struct SignatureCache {
    /// Maps signature -> when it was first seen
    signatures: HashMap<String, Instant>,
    /// How long to keep signatures in cache (2x timestamp window for safety)
    ttl: Duration,
}

impl SignatureCache {
    /// Create a new signature cache with 2-minute TTL (2x the 60s timestamp window)
    pub fn new() -> Self {
        Self {
            signatures: HashMap::new(),
            ttl: Duration::from_secs(120), // 2 minutes
        }
    }

    /// Check if a signature has been used before (replay attack detection)
    /// Also automatically cleans up old entries
    /// Returns true if this is a replay (signature already seen)
    pub fn is_replay(&mut self, signature: &str) -> bool {
        let now = Instant::now();
        
        // Clean up old signatures first
        self.cleanup(now);
        
        // Check if signature is in cache
        if self.signatures.contains_key(signature) {
            tracing::warn!(signature = %signature, "Replay attack detected");
            return true;
        }
        
        false
    }

    /// Add a signature to the cache
    pub fn add(&mut self, signature: &str) {
        let now = Instant::now();
        self.signatures.insert(signature.to_string(), now);
        
        tracing::debug!(
            signature = %signature,
            cache_size = self.signatures.len(),
            "Signature added to cache"
        );
    }

    /// Remove signatures older than TTL
    fn cleanup(&mut self, now: Instant) {
        let before_count = self.signatures.len();
        
        self.signatures.retain(|_, &mut first_seen| {
            now.duration_since(first_seen) < self.ttl
        });
        
        let removed = before_count - self.signatures.len();
        if removed > 0 {
            tracing::debug!(
                removed = removed,
                remaining = self.signatures.len(),
                "Cleaned up old signatures from cache"
            );
        }
    }

    /// Get current cache size (for monitoring)
    pub fn size(&self) -> usize {
        self.signatures.len()
    }
}

impl Default for SignatureCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_replay_detection() {
        let mut cache = SignatureCache::new();
        let sig = "0x1234567890abcdef";

        // First time - not a replay
        assert!(!cache.is_replay(sig));

        // Add to cache
        cache.add(sig);

        // Second time - is a replay
        assert!(cache.is_replay(sig));
    }

    #[test]
    fn test_cleanup() {
        let mut cache = SignatureCache {
            signatures: HashMap::new(),
            ttl: Duration::from_millis(100),
        };

        let sig1 = "0xaaaa";
        let sig2 = "0xbbbb";

        cache.add(sig1);
        assert_eq!(cache.size(), 1);

        // Wait for TTL to expire
        thread::sleep(Duration::from_millis(150));

        let now = Instant::now();

        cache.add(sig2);

        cache.cleanup(now);
        assert_eq!(cache.size(), 1); // sig1 should be cleaned up

        // sig1 should not be a replay anymore (it was cleaned)
        assert!(!cache.is_replay(sig1));
        // sig2 should be a replay
        assert!(cache.is_replay(sig2));
    }
}

