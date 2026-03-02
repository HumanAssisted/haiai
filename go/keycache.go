package haisdk

import (
	"sync"
	"time"
)

// keyCacheTTL is the time-to-live for cached agent keys (5 minutes).
const keyCacheTTL = 5 * time.Minute

// cachedKey stores a PublicKeyInfo with its cache timestamp.
type cachedKey struct {
	info     *PublicKeyInfo
	cachedAt time.Time
}

// keyCache is a thread-safe agent key cache.
type keyCache struct {
	mu      sync.RWMutex
	entries map[string]cachedKey
}

// newKeyCache creates a new empty key cache.
func newKeyCache() *keyCache {
	return &keyCache{entries: make(map[string]cachedKey)}
}

// get returns a cached key if it exists and hasn't expired, or nil.
func (c *keyCache) get(key string) *PublicKeyInfo {
	c.mu.RLock()
	entry, ok := c.entries[key]
	c.mu.RUnlock()

	if !ok {
		return nil
	}
	if time.Since(entry.cachedAt) >= keyCacheTTL {
		c.mu.Lock()
		delete(c.entries, key)
		c.mu.Unlock()
		return nil
	}
	return entry.info
}

// set stores a key in the cache with the current timestamp.
func (c *keyCache) set(key string, info *PublicKeyInfo) {
	c.mu.Lock()
	c.entries[key] = cachedKey{info: info, cachedAt: time.Now()}
	c.mu.Unlock()
}

// clear removes all entries from the cache.
func (c *keyCache) clear() {
	c.mu.Lock()
	c.entries = make(map[string]cachedKey)
	c.mu.Unlock()
}
