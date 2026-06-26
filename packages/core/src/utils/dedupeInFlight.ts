/**
 * Concurrent-fetch deduplication.
 *
 * Two callers asking for the same key at the same time end up
 * sharing one Promise rather than one of them busy-polling for
 * the other to settle. Replaces the `while (loading.has(key))
 * await setTimeout(16)` pattern that several stores were using.
 *
 * The registry is tied to a single store instance — pass in your
 * own Map so each store keeps its own in-flight set and one
 * key's failure doesn't poison another store's cache.
 */

export async function dedupeInFlight<K, V>(
  registry: Map<K, Promise<V>>,
  key: K,
  fetcher: () => Promise<V>,
): Promise<V> {
  const existing = registry.get(key)
  if (existing) return existing
  const promise = (async () => {
    try {
      return await fetcher()
    } finally {
      registry.delete(key)
    }
  })()
  registry.set(key, promise)
  return promise
}
