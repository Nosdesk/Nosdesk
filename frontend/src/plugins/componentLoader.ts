/**
 * Plugin Component Loader
 *
 * Loads plugin bundles (ES modules) from the backend, verifies their
 * SHA-256 against the hash the server recorded at sign/install time,
 * and creates async Vue components that render plugin UI in slots.
 *
 * The integrity check is the browser's independent safety net: even
 * if the backend is compromised and serves different bytes than what
 * was signed, the hash mismatch keeps the module from executing.
 * Fetched bytes are handed to `import()` via a blob URL so there's
 * exactly one path from network to execution, and the hash check
 * gates it.
 */

import { defineAsyncComponent, type Component } from 'vue';
import { getLoadedPlugin } from './loader';
import { logger } from '@/utils/logger';
import PluginLoading from './components/PluginLoading.vue';
import PluginError from './components/PluginError.vue';

// =============================================================================
// Types
// =============================================================================

export interface PluginModule {
  [componentName: string]: Component;
}

// =============================================================================
// Module Cache
// =============================================================================

// Cache plugin bundles to avoid re-fetching. Keyed by
// `<uuid>:<bundle_hash>` so a hash change (new bundle uploaded,
// registry version bump) invalidates the cache automatically — we
// never serve stale code that disagrees with the backend's current
// integrity claim. When a new hash lands for a plugin we prune
// any stale `<uuid>:*` entries so the map doesn't grow unbounded
// across bundle rotations.
const moduleCache = new Map<string, Promise<PluginModule>>();

// Plugins whose last load attempt failed. Entries age out after
// FAILURE_TTL_MS so a transient blip (auth expiry, deploy flash)
// doesn't permanently disable rendering until a manual reload.
const FAILURE_TTL_MS = 30_000;
const failedPlugins = new Map<string, number>();

function markFailed(uuid: string): void {
  failedPlugins.set(uuid, Date.now());
}

function isRecentFailure(uuid: string): boolean {
  const ts = failedPlugins.get(uuid);
  if (ts === undefined) return false;
  if (Date.now() - ts > FAILURE_TTL_MS) {
    failedPlugins.delete(uuid);
    return false;
  }
  return true;
}

// =============================================================================
// Bundle Loading
// =============================================================================

/**
 * Fetch a plugin bundle, verify its SHA-256 against the hash the
 * backend recorded at sign/install time, and dynamic-import the
 * verified bytes.
 *
 * The integrity check defends against a scenario where the backend
 * serves bytes that don't match what it previously signed — disk
 * corruption, an attacker swapping the staged bundle, or the
 * compiled JS drifting from the signed archive. A mismatch throws
 * and the module is never evaluated.
 */
async function loadPluginBundle(
  pluginUuid: string,
  expectedHash: string
): Promise<PluginModule> {
  const url = `/api/plugins/${pluginUuid}/bundle`;

  logger.debug(`Loading plugin bundle: ${pluginUuid}`, { url });

  try {
    const response = await fetch(url, { credentials: 'same-origin' });
    if (!response.ok) {
      throw new Error(`Plugin bundle HTTP ${response.status}`);
    }
    const bytes = await response.arrayBuffer();

    const actualHash = await sha256Hex(bytes);
    if (actualHash !== expectedHash) {
      throw new Error(
        `Bundle integrity check failed: expected ${expectedHash}, got ${actualHash}`
      );
    }

    // Hand the already-verified bytes to dynamic import via a blob
    // URL. This is the only path that loads plugin code — nothing
    // else calls `import()` on plugin URLs — so every execution
    // flows through the hash check above.
    const blob = new Blob([bytes], { type: 'application/javascript' });
    const blobUrl = URL.createObjectURL(blob);
    let imported: { default?: unknown };
    try {
      imported = await import(/* @vite-ignore */ blobUrl);
    } finally {
      URL.revokeObjectURL(blobUrl);
    }

    if (!imported.default) {
      throw new Error('Plugin bundle must have a default export');
    }

    const module = imported.default as PluginModule;
    logger.info(`Loaded plugin bundle: ${pluginUuid}`, {
      components: Object.keys(module),
    });
    return module;
  } catch (error) {
    logger.error(`Failed to load plugin bundle: ${pluginUuid}`, { error });
    markFailed(pluginUuid);
    throw error;
  }
}

async function sha256Hex(bytes: ArrayBuffer): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', bytes);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}

/**
 * Get a cached or fresh plugin bundle. Keyed by (uuid, expectedHash)
 * so bumping the hash is a cache bust by construction.
 */
function getPluginBundle(
  pluginUuid: string,
  expectedHash: string
): Promise<PluginModule> {
  const key = `${pluginUuid}:${expectedHash}`;
  if (!moduleCache.has(key)) {
    // Evict older hash variants for this plugin before inserting.
    // Prevents unbounded growth when the registry rotates a
    // plugin's hash and the old entry would otherwise linger.
    const prefix = `${pluginUuid}:`;
    for (const existing of moduleCache.keys()) {
      if (existing !== key && existing.startsWith(prefix)) {
        moduleCache.delete(existing);
      }
    }
    moduleCache.set(key, loadPluginBundle(pluginUuid, expectedHash));
  }
  return moduleCache.get(key)!;
}

/**
 * Preload a plugin's bundle so it's cached before any component renders.
 * Called during plugin loading to eliminate the async loading flash.
 * Silently skips plugins that have no recorded bundle_hash — we
 * fail closed rather than loading unverified bytes.
 */
export async function preloadPluginBundle(pluginUuid: string): Promise<void> {
  const loaded = getLoadedPlugin(pluginUuid);
  if (!loaded || !loaded.plugin.bundle_hash) {
    return;
  }
  try {
    await getPluginBundle(pluginUuid, loaded.plugin.bundle_hash);
  } catch {
    // Errors are already tracked in failedPlugins set
  }
}

// =============================================================================
// Component Creation
// =============================================================================

/**
 * Create an async Vue component that loads from a plugin bundle.
 *
 * @param pluginUuid - The UUID of the plugin
 * @param componentName - The name of the component to load from the bundle
 * @returns An async Vue component
 */
export function createPluginComponent(
  pluginUuid: string,
  componentName: string
): Component {
  logger.debug(`Creating async component: ${componentName} for plugin ${pluginUuid}`);

  return defineAsyncComponent({
    loader: async () => {
      const loadedPlugin = getLoadedPlugin(pluginUuid);

      if (!loadedPlugin) {
        throw new Error(`Plugin not loaded: ${pluginUuid}`);
      }

      // Security: community plugins cannot load components. Local,
      // verified, and official are allowed — local plugins were
      // explicitly signed and installed by an admin with CLI
      // access, so the trust decision is an act of administration
      // rather than a registry click.
      if (loadedPlugin.plugin.trust_level === 'community') {
        throw new Error('Community plugins cannot render components');
      }

      if (!loadedPlugin.plugin.bundle_uploaded_at) {
        throw new Error('Plugin has no uploaded bundle');
      }
      if (!loadedPlugin.plugin.bundle_hash) {
        throw new Error('Plugin bundle has no recorded hash; refusing to load');
      }

      const module = await getPluginBundle(pluginUuid, loadedPlugin.plugin.bundle_hash);

      if (!module[componentName]) {
        throw new Error(`Component not found in bundle: ${componentName}`);
      }

      return module[componentName];
    },
    loadingComponent: PluginLoading,
    errorComponent: PluginError,
    timeout: 10000,
    delay: 0,
  });
}

/**
 * Check if a plugin can render components
 *
 * @param pluginUuid - The UUID of the plugin
 * @returns Whether the plugin can render components
 */
export function canRenderComponent(pluginUuid: string): boolean {
  const loadedPlugin = getLoadedPlugin(pluginUuid);

  if (!loadedPlugin) {
    return false;
  }

  // Community plugins are registry-distributed and only gain
  // component-rendering privileges after the tier-specific confirm
  // flow; everything else (official, verified, local) can render.
  if (loadedPlugin.plugin.trust_level === 'community') {
    return false;
  }

  // Must have a bundle with an integrity hash. Missing hash means
  // we can't verify what the backend will serve, so fail closed.
  if (!loadedPlugin.plugin.bundle_uploaded_at || !loadedPlugin.plugin.bundle_hash) {
    return false;
  }

  // Hide if the last load attempt failed within FAILURE_TTL_MS.
  // Stale failures are purged by isRecentFailure so a transient
  // outage doesn't leave the plugin disabled forever.
  if (isRecentFailure(pluginUuid)) {
    return false;
  }

  return true;
}

/**
 * Clear cached bundles for a specific plugin across all known hash
 * keys. Called after a bundle update to force a fresh fetch +
 * integrity check on the next render.
 */
export function clearPluginCache(pluginUuid: string): void {
  const prefix = `${pluginUuid}:`;
  for (const key of moduleCache.keys()) {
    if (key.startsWith(prefix)) {
      moduleCache.delete(key);
    }
  }
  failedPlugins.delete(pluginUuid);
  logger.debug(`Cleared plugin cache: ${pluginUuid}`);
}

/**
 * Clear all cached plugin bundles
 */
export function clearAllPluginCaches(): void {
  moduleCache.clear();
  failedPlugins.clear();
  logger.debug('Cleared all plugin caches');
}

/* Surface for completeness: `failedPlugins` now ages entries out
 * automatically via FAILURE_TTL_MS, so the explicit clear above
 * covers both maps and no external caller has to call this after a
 * transient error. */
