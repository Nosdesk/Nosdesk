// Registry of the LIVE plugin API instances a plugin's event handlers actually
// land on, so the event dispatcher can reach them.
//
// `createPluginAPI` returns a fresh instance per call, and context is
// per-instance (a plugin in two slots has two independent contexts). So
// `api.on(...)` handlers register on the specific instance the render path
// created — the in-process `PluginAPI` a sandboxed plugin's `createHostApiImpl`
// wraps behind the bridge (there is no in-process render path any more). The
// dispatcher must
// iterate exactly those live instances (not a throwaway of its own), which is
// what this registry provides. Each mounted instance registers on setup and
// unregisters on unmount; a plugin rendered in N places has N live instances and
// receives each event N times (once per instance), which is correct.
import type { PluginAPI } from './api';

const live = new Map<string, Set<PluginAPI>>();

/** Register a live instance; returns an unregister fn to call on unmount. */
export function registerPluginInstance(uuid: string, api: PluginAPI): () => void {
  let set = live.get(uuid);
  if (!set) {
    set = new Set();
    live.set(uuid, set);
  }
  set.add(api);
  return () => {
    const s = live.get(uuid);
    if (!s) return;
    s.delete(api);
    if (s.size === 0) live.delete(uuid);
  };
}

/** Visit every live instance across all plugins (uuid + its API instance). */
export function forEachLiveInstance(cb: (uuid: string, api: PluginAPI) => void): void {
  for (const [uuid, set] of live) {
    for (const api of set) cb(uuid, api);
  }
}
