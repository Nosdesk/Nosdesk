// The typed contract a sandboxed Nosdesk plugin codes against.
//
// This is the boundary-safe shape of the host API: every method is async (a
// postMessage round-trip over the Comlink bridge) and every value is
// structured-clone-safe. It differs from the app's internal `PluginAPI` in the
// four places the process boundary forces (see docs/plugin-sandbox-plan.md):
//   - `fetch` returns a plain object, not a live `Response`.
//   - `context` is a snapshot passed to `mount` (+ `onContextChange`), not a
//     live reactive getter.
//   - `on(...)`'s handler is a callback wrapped by the SDK with Comlink.proxy.
//   - the host-internal `_setContext` / `_getEventHandlers` are not exposed.
//
// Domain types are re-exported from `@nosdesk/core/types` so plugin authors
// import only from `@nosdesk/plugin-sdk`.
import type {
  Ticket,
  Asset,
  CollectionRow,
  CollectionListResponse,
  PluginEvent,
} from '@nosdesk/core/types';

export type { Ticket, Asset, CollectionRow, CollectionListResponse, PluginEvent };

/** A comment a plugin adds to a ticket. */
export interface PluginComment {
  content: string;
  is_internal?: boolean;
}

/** A ticket attachment as exposed to plugins. */
export interface PluginAttachment {
  id: number;
  name: string;
  url: string;
  mimeType: string;
  size: number;
}

export interface PluginFetchOptions {
  method?: string;
  headers?: Record<string, string>;
  body?: unknown;
  /** How to encode `body` (default `json`). */
  content_type?: 'json' | 'form';
}

/** A plain, structured-clone-safe response from `api.fetch`, NOT a live
 * `Response` (which cannot cross the bridge). */
export interface PluginFetchResponse {
  status: number;
  headers: Record<string, string>;
  body: string;
}

/** A host-event handler. The SDK wraps it with `Comlink.proxy` so it can cross
 * the bridge. */
export type PluginEventHandler = (data: unknown) => void | Promise<void>;

/** CRUD over one plugin collection. */
export interface PluginCollection {
  create(data: Record<string, unknown>): Promise<CollectionRow | null>;
  get(uuid: string): Promise<CollectionRow | null>;
  update(uuid: string, data: Record<string, unknown>): Promise<CollectionRow | null>;
  delete(uuid: string): Promise<boolean>;
  list(params?: {
    limit?: number;
    offset?: number;
    filter?: string;
    sort_by?: string;
    sort_order?: string;
  }): Promise<CollectionListResponse>;
}

/**
 * The host API a sandboxed plugin calls over the Comlink bridge. Each call is a
 * round-trip; the host enforces the plugin's manifest permissions per call. A
 * method rejects (or the sub-API is absent) when a permission isn't granted.
 */
export interface HostApi {
  /** Runtime API version (semver); the major is the breaking-change signal. */
  readonly version: string;
  readonly plugin: { uuid: string; name: string; displayName: string; version: string };

  tickets: {
    get(id: number): Promise<Ticket | null>;
    list(): Promise<Ticket[]>;
    addComment(ticketId: number, comment: PluginComment): Promise<boolean>;
  };
  devices: {
    get(id: number): Promise<Asset | null>;
    list(): Promise<Asset[]>;
  };
  attachments: {
    list(ticketId: number): Promise<PluginAttachment[]>;
    getBase64(
      attachmentId: number,
      ticketId: number,
    ): Promise<{ data: string; name: string; mimeType: string } | null>;
  };
  /** Outbound HTTP via the host's SSRF-guarded, manifest-allowlisted proxy. */
  fetch(url: string, options?: PluginFetchOptions): Promise<PluginFetchResponse | null>;
  storage: {
    get<T>(key: string): Promise<T | null>;
    set<T>(key: string, value: T): Promise<boolean>;
    delete(key: string): Promise<boolean>;
  };
  /** Access a named collection's CRUD (returns a Comlink-proxied sub-API). */
  collections(name: string): Promise<PluginCollection>;
  /** Subscribe to a host event; resolves to an async unsubscribe. */
  on(event: PluginEvent, handler: PluginEventHandler): Promise<() => Promise<void>>;
  notify(message: string, type?: 'info' | 'success' | 'warning' | 'error'): Promise<void>;
  ui: { isPrinting(): Promise<boolean> };
}

/** The host-provided context, snapshotted into the iframe. It is not a live
 * object (it can't cross postMessage reactively); updates arrive via
 * `onContextChange`. */
export interface PluginContext {
  ticket: Ticket | null;
  device: Asset | null;
  /** Which of the plugin's manifest components this mount is rendering. A bundle
   * may declare several (different slots); the plugin switches its UI on `name`
   * (the manifest `components` map key) / `slot`. */
  component: { name: string; slot: string };
  /** Monotonic counter for this component's declared `action` (a host menu
   * trigger). Increments on each activation; absent if the component declares no
   * action. The plugin reacts (e.g. opens a panel) when it changes. */
  actionActivated?: number;
}

/** What a plugin's `mount` may return to get granular updates instead of a
 * re-mount: `update` is called with each new context, `unmount` on teardown.
 * Both optional — omitting `update` falls back to unmount + re-mount. */
export interface PluginInstance {
  update?(context: PluginContext): void;
  unmount?(): void;
}

/** A plugin bundle's default export: the framework-agnostic entry. The runtime
 * calls `mount` once the bridge is ready. Return nothing or a teardown function
 * for a simple plugin (re-mounted on context change), or a `PluginInstance` to
 * handle context updates in place. */
export interface PluginModule {
  mount(rootEl: HTMLElement, api: import('comlink').Remote<HostApi>, context: PluginContext):
    | void
    | (() => void)
    | PluginInstance;
}
