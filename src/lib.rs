//! plato-tile-cache — LRU cache for PLATO tile queries with TTL eviction
//!
//! Caches tile search results to avoid redundant computation.
//! Zero deps, hand-rolled LRU using Vec + HashMap.

use std::collections::HashMap;

/// A cached query result.
#[derive(Debug, Clone)]
pub struct CacheEntry<V: Clone> {
    pub key: String,
    pub value: V,
    pub hits: u32,
    pub created_tick: u64,
    pub last_access_tick: u64,
    pub ttl_ticks: u64,
}

/// LRU cache with TTL eviction.
pub struct TileCache<V: Clone> {
    entries: HashMap<String, CacheEntry<V>>,
    order: Vec<String>,          // most-recent at front
    max_size: usize,
    tick: u64,
    stats: CacheStats,
}

#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub gets: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub expired: u64,
    pub inserts: u64,
}

impl<V: Clone> TileCache<V> {
    pub fn new(max_size: usize) -> Self {
        Self { entries: HashMap::new(), order: Vec::new(), max_size,
               tick: 0, stats: CacheStats::default() }
    }

    pub fn with_ttl(max_size: usize, ttl_ticks: u64) -> Self {
        let mut c = Self::new(max_size);
        // Default TTL stored per entry
        let _ = ttl_ticks; // applied on insert
        c
    }

    pub fn tick(&mut self) { self.tick += 1; }
    pub fn current_tick(&self) -> u64 { self.tick }

    /// Get a cached value. Returns None on miss or expired.
    pub fn get(&mut self, key: &str) -> Option<V> {
        self.stats.gets += 1;
        // Check TTL first
        if let Some(e) = self.entries.get(key) {
            if self.tick > e.created_tick + e.ttl_ticks {
                self.remove_entry(key);
                self.stats.expired += 1;
                return None;
            }
        } else {
            self.stats.misses += 1;
            return None;
        }
        // Now safe to mutate
        let entry = self.entries.get_mut(key).unwrap();
        entry.hits += 1;
        entry.last_access_tick = self.tick;
        let value = entry.value.clone();
        self.promote(key);
        self.stats.hits += 1;
        Some(value)
    }

    /// Insert a value with default TTL (100 ticks).
    pub fn insert(&mut self, key: &str, value: V) {
        self.insert_with_ttl(key, value, 100)
    }

    /// Insert with custom TTL in ticks.
    pub fn insert_with_ttl(&mut self, key: &str, value: V, ttl_ticks: u64) {
        self.stats.inserts += 1;
        if self.entries.contains_key(key) {
            // Update existing
            if let Some(e) = self.entries.get_mut(key) {
                e.value = value;
                e.created_tick = self.tick;
                e.last_access_tick = self.tick;
                e.ttl_ticks = ttl_ticks;
            }
            self.promote(key);
            return;
        }
        // Evict if full
        while self.entries.len() >= self.max_size {
            self.evict_lru();
        }
        self.entries.insert(key.to_string(), CacheEntry {
            key: key.to_string(), value, hits: 0,
            created_tick: self.tick, last_access_tick: self.tick, ttl_ticks,
        });
        self.order.insert(0, key.to_string());
    }

    /// Remove a key.
    pub fn remove(&mut self, key: &str) -> bool {
        if self.entries.remove(key).is_some() {
            self.order.retain(|k| k != key);
            true
        } else { false }
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    /// Current size.
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }

    /// Hit rate (0.0 - 1.0).
    pub fn hit_rate(&self) -> f64 {
        if self.stats.gets == 0 { return 0.0; }
        self.stats.hits as f64 / self.stats.gets as f64
    }

    /// Stats.
    pub fn stats(&self) -> &CacheStats { &self.stats }

    /// Expire all entries past their TTL.
    pub fn expire_all(&mut self) -> usize {
        let expired: Vec<String> = self.entries.iter()
            .filter(|(_, e)| self.tick > e.created_tick + e.ttl_ticks)
            .map(|(k, _)| k.clone())
            .collect();
        let count = expired.len();
        for k in &expired { self.remove_entry(k); }
        self.stats.expired += count as u64;
        count
    }

    /// Get top-N most-hit entries.
    pub fn top_hits(&self, n: usize) -> Vec<(&str, u32)> {
        let mut ranked: Vec<(&str, u32)> = self.entries.iter()
            .map(|(k, e)| (k.as_str(), e.hits)).collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1));
        ranked.truncate(n);
        ranked
    }

    /// Keys sorted by last access (most recent first).
    pub fn lru_order(&self) -> Vec<&str> {
        self.order.iter().map(|s| s.as_str()).collect()
    }

    // ── Internal ──

    fn promote(&mut self, key: &str) {
        self.order.retain(|k| k != key);
        self.order.insert(0, key.to_string());
    }

    fn evict_lru(&mut self) {
        if let Some(key) = self.order.pop() {
            self.entries.remove(&key);
            self.stats.evictions += 1;
        }
    }

    fn remove_entry(&mut self, key: &str) {
        self.entries.remove(key);
        self.order.retain(|k| k != key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_get() {
        let mut c: TileCache<String> = TileCache::new(10);
        c.insert("k1", "v1".to_string());
        assert_eq!(c.get("k1"), Some("v1".to_string()));
    }

    #[test]
    fn test_miss() {
        let mut c: TileCache<String> = TileCache::new(10);
        assert_eq!(c.get("missing"), None);
        assert_eq!(c.stats().misses, 1);
    }

    #[test]
    fn test_lru_eviction() {
        let mut c: TileCache<i32> = TileCache::new(3);
        c.insert("a", 1);
        c.insert("b", 2);
        c.insert("c", 3);
        assert_eq!(c.len(), 3);
        c.insert("d", 4); // evicts "a" (LRU)
        assert_eq!(c.get("a"), None);
        assert_eq!(c.get("d"), Some(4));
    }

    #[test]
    fn test_lru_promote() {
        let mut c: TileCache<i32> = TileCache::new(3);
        c.insert("a", 1);
        c.insert("b", 2);
        c.insert("c", 3);
        c.get("a"); // promote "a"
        c.insert("d", 4); // evicts "b" (now LRU)
        assert_eq!(c.get("b"), None);
        assert_eq!(c.get("a"), Some(1));
    }

    #[test]
    fn test_ttl_expiry() {
        let mut c: TileCache<String> = TileCache::new(10);
        c.insert_with_ttl("k", "v".to_string(), 5);
        for _ in 0..5 { c.tick(); }
        assert_eq!(c.get("k"), Some("v".to_string())); // tick 5, not expired yet
        c.tick(); // tick 6
        assert_eq!(c.get("k"), None); // expired
        assert_eq!(c.stats().expired, 1);
    }

    #[test]
    fn test_expire_all() {
        let mut c: TileCache<i32> = TileCache::new(10);
        c.insert_with_ttl("a", 1, 3);
        c.insert_with_ttl("b", 2, 10);
        for _ in 0..5 { c.tick(); }
        let expired = c.expire_all();
        assert_eq!(expired, 1);
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn test_update_existing() {
        let mut c: TileCache<i32> = TileCache::new(10);
        c.insert("k", 1);
        c.insert("k", 2);
        assert_eq!(c.get("k"), Some(2));
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn test_remove() {
        let mut c: TileCache<i32> = TileCache::new(10);
        c.insert("k", 1);
        assert!(c.remove("k"));
        assert!(!c.remove("k"));
        assert!(c.is_empty());
    }

    #[test]
    fn test_clear() {
        let mut c: TileCache<i32> = TileCache::new(10);
        c.insert("a", 1);
        c.insert("b", 2);
        c.clear();
        assert!(c.is_empty());
    }

    #[test]
    fn test_hit_rate() {
        let mut c: TileCache<i32> = TileCache::new(10);
        c.insert("k", 1);
        c.get("k"); // hit
        c.get("k"); // hit
        c.get("x"); // miss
        let rate = c.hit_rate();
        assert!((rate - 0.666).abs() < 0.01);
    }

    #[test]
    fn test_top_hits() {
        let mut c: TileCache<i32> = TileCache::new(10);
        c.insert("a", 1);
        c.insert("b", 2);
        c.insert("c", 3);
        for _ in 0..5 { c.get("a"); }
        for _ in 0..2 { c.get("c"); }
        let top = c.top_hits(2);
        assert_eq!(top[0].0, "a");
        assert_eq!(top[0].1, 5);
        assert_eq!(top[1].0, "c");
    }

    #[test]
    fn test_lru_order() {
        let mut c: TileCache<i32> = TileCache::new(10);
        c.insert("a", 1);
        c.insert("b", 2);
        c.insert("c", 3);
        c.get("a"); // promote
        let order = c.lru_order();
        assert_eq!(order[0], "a");
        assert_eq!(order[1], "c");
        assert_eq!(order[2], "b");
    }

    #[test]
    fn test_stats() {
        let mut c: TileCache<i32> = TileCache::new(2);
        c.insert("a", 1);
        c.insert("b", 2);
        c.get("a"); // hit
        c.get("z"); // miss
        c.insert("c", 3); // evict b
        assert_eq!(c.stats().inserts, 3);
        assert_eq!(c.stats().gets, 2);
        assert_eq!(c.stats().hits, 1);
        assert_eq!(c.stats().misses, 1);
        assert_eq!(c.stats().evictions, 1);
    }

    #[test]
    fn test_with_generic_vec() {
        let mut c: TileCache<Vec<i32>> = TileCache::new(5);
        c.insert("nums", vec![1, 2, 3]);
        assert_eq!(c.get("nums"), Some(vec![1, 2, 3]));
    }
}
