import * as Comlink from 'comlink';

import type { HostApi, PluginContext } from './types';

/**
 * The host end of the plugin bridge. `createRemoteHostApi` exposes a `HostApi`
 * over a fresh `MessageChannel` and returns the far `port` for the caller to
 * transfer into the sandboxed iframe (via `postInit`, which the runtime's
 * `connectToHost` awaits). The bridge is pure transport: per-call permission
 * enforcement and the workspace pin live inside the `impl` the host passes.
 */
export interface HostBridge {
  /** Transfer this into the iframe with the init message; never expose it
   * elsewhere (holding the port IS the capability). */
  readonly port: MessagePort;
  /** Close the channel and stop serving the API. */
  dispose(): void;
}

/** Expose `impl` over a fresh channel; return the port to hand to the iframe. */
export function createRemoteHostApi(impl: HostApi): HostBridge {
  const channel = new MessageChannel();
  Comlink.expose(impl, channel.port1);
  return {
    port: channel.port2,
    dispose() {
      channel.port1.close();
      channel.port2.close();
    },
  };
}

/**
 * Post the init handshake to a sandboxed plugin iframe: the transferred port +
 * the initial context. `targetOrigin` is `*` by design, an opaque sandboxed
 * frame has origin `"null"`, so the capability is the transferred port (only the
 * host can post to this specific `contentWindow`), not an origin string. Call
 * once the iframe has loaded. Mirrors the message the SDK's `connectToHost`
 * awaits.
 */
export function postInit(target: Window, bridge: HostBridge, context: PluginContext): void {
  target.postMessage({ type: 'nosdesk-plugin-init', context }, '*', [bridge.port]);
}

/** Push a fresh context snapshot to an already-connected plugin iframe. */
export function postContext(target: Window, context: PluginContext): void {
  target.postMessage({ type: 'nosdesk-plugin-context', context }, '*');
}
