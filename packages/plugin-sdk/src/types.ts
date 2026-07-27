// The typed contract a sandboxed Nosdesk plugin codes against.
//
// This is the boundary-safe shape of the host API: every method is async (a
// postMessage round-trip over the Comlink bridge) and every value is
// structured-clone-safe. It differs from the app's internal `PluginAPI` in the
// four places the process boundary forces (see docs/plugin-sandbox-plan.md):
//   - `fetch` returns a plain object, not a live `Response`.
//   - `context` is a snapshot passed to `mount` (+ `onContextChange`), not a
//     live reactive getter.
//   - `on(...)` takes a plain handler; the runtime keeps it local and proxies
//     the bridge callback itself (see `PluginHostApi`), so plugins never touch
//     Comlink.proxy.
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

/** The writable-field subset a plugin may patch on a ticket (`ticket:write`).
 * Deliberately narrow, not the full ticket. */
export interface PluginTicketPatch {
  title?: string;
  priority?: string;
  workflow_state_id?: number;
  assignee?: string | null;
}

/** The writable-field subset a plugin may patch on an asset (`asset:write`). */
export interface PluginAssetPatch {
  name?: string;
  status?: string;
  location?: string | null;
}

/** A safe user projection for `user:read` — identity only, workspace members. */
export interface PluginUser {
  uuid: string;
  name: string;
  email: string;
  avatarUrl: string | null;
  role: string;
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
/** The payload an event handler receives: the sync action's type, the affected
 * aggregate's id, and its entity projection in `data`. A subset of the host's
 * internal sync action — the fields a plugin can rely on. */
export interface PluginEventPayload {
  event_type: string;
  aggregate_id: string;
  data: unknown;
}

export type PluginEventHandler = (payload: PluginEventPayload) => void | Promise<void>;

/** Query for `api.users.list` — mirrors the validated server params. */
export interface PluginUserQuery {
  /** Free-text over name/email (server truncates to 100 chars). */
  search?: string;
  /** Filter by role. */
  role?: 'admin' | 'agent' | 'user';
  /** Page size, 1..100 (default 25). */
  limit?: number;
  /** 1-based page. */
  page?: number;
  sortBy?: 'name' | 'email' | 'role' | 'created_at' | 'updated_at';
  sortOrder?: 'asc' | 'desc';
}

/** A page of workspace members from `api.users.list`. */
export interface PluginUserList {
  users: PluginUser[];
  /** Total matching the query across all pages. */
  total: number;
}

/** A workspace workflow state — the values a ticket's `workflow_state_id` can
 * take. `category` is the fixed system bucket; `name`/`color` are configurable. */
export interface PluginWorkflowState {
  id: number;
  name: string;
  category: string;
  color: string;
  position: number;
  is_default: boolean;
}

/** The fixed ticket priority scale. */
export type PluginPriority = 'low' | 'medium' | 'high';

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
 * round-trip; the host enforces the plugin's consented permissions per call.
 *
 * Failure contract: a call THROWS a `PluginApiError` (recover with
 * `asPluginApiError`, branch on `.code`: `denied` | `invalid` | `rate_limited` |
 * `timeout` | `upstream`). `null` is returned only for a genuine not-found — a
 * `get` of a resource that doesn't exist. So `null` means "not there"; a throw
 * means "couldn't".
 */
export interface HostApi {
  /** Runtime API version (semver); the major is the breaking-change signal. */
  readonly version: string;
  readonly plugin: { uuid: string; name: string; displayName: string; version: string };

  tickets: {
    get(id: number): Promise<Ticket | null>;
    list(): Promise<Ticket[]>;
    addComment(ticketId: number, comment: PluginComment): Promise<boolean>;
    /** Patch a ticket (`ticket:write`). Acts as the current user. */
    update(id: number, patch: PluginTicketPatch): Promise<Ticket | null>;
    /** Delete a ticket (`ticket:delete`). Acts as the current user. */
    delete(id: number): Promise<boolean>;
    /** The workspace's workflow states — the valid `workflow_state_id` values for
     * `update` (`ticket:read`). */
    workflowStates(): Promise<PluginWorkflowState[]>;
    /** The fixed priority scale for `update`'s `priority` field. */
    priorities(): Promise<PluginPriority[]>;
  };
  assets: {
    get(id: number): Promise<Asset | null>;
    list(): Promise<Asset[]>;
    /** Patch an asset (`asset:write`). Acts as the current user. */
    update(id: number, patch: PluginAssetPatch): Promise<Asset | null>;
  };
  users: {
    /** The current (acting) user's identity projection. Ambient — no permission
     * (the plugin already runs in this user's session). */
    me(): Promise<PluginUser | null>;
    /** Fetch a workspace member's identity projection (`user:read`). */
    get(uuid: string): Promise<PluginUser | null>;
    /** Search workspace members (`user:read`). */
    list(query?: PluginUserQuery): Promise<PluginUserList>;
  };
  attachments: {
    list(ticketId: number): Promise<PluginAttachment[]>;
    getBase64(
      attachmentId: number,
      ticketId: number,
    ): Promise<{ data: string; name: string; mimeType: string } | null>;
  };
  /** Outbound HTTP via the host's SSRF-guarded, manifest-allowlisted proxy.
   * Throws a `PluginApiError` on denial / invalid URL / upstream failure. */
  fetch(url: string, options?: PluginFetchOptions): Promise<PluginFetchResponse>;
  storage: {
    get<T>(key: string): Promise<T | null>;
    set<T>(key: string, value: T): Promise<boolean>;
    delete(key: string): Promise<boolean>;
  };
  /** Access a named collection's CRUD (returns a Comlink-proxied sub-API). */
  collections(name: string): Promise<PluginCollection>;
  /** Subscribe to a host event; resolves to an async unsubscribe. */
  on(event: PluginEvent, handler: PluginEventHandler): Promise<() => Promise<void>>;
  settings: {
    /** Read one of the plugin's admin-configured settings. Secrets are redacted
     * (a `secret`-type setting's value is always `null` — the egress proxy
     * injects them server-side); `null` also for an unset key. */
    get(key: string): Promise<unknown | null>;
  };
  notify(message: string, type?: 'info' | 'success' | 'warning' | 'error'): Promise<void>;
  ui: { isPrinting(): Promise<boolean> };
}

/**
 * The host API as a plugin actually receives it in `mount`. Identical to the
 * Comlink-remote `HostApi` except for `on`: the runtime intercepts it, keeps the
 * handler local to the iframe, and registers its own Comlink-proxied dispatcher
 * with the host. That's what makes events work at all — a plugin's own bundled
 * Comlink has a different `proxyMarker` than the runtime's `api`, so a
 * plugin-proxied callback can't cross the bridge (it fails to structured-clone).
 * With this, the plugin just passes a plain function and gets back an
 * unsubscribe. */
export type PluginHostApi = Omit<import('comlink').Remote<HostApi>, 'on'> & {
  on(event: PluginEvent, handler: PluginEventHandler): Promise<() => void>;
};

/** The host-provided context, snapshotted into the iframe. It is not a live
 * object (it can't cross postMessage reactively); updates arrive via
 * `onContextChange`. */
export interface PluginContext {
  ticket: Ticket | null;
  asset: Asset | null;
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
  mount(rootEl: HTMLElement, api: PluginHostApi, context: PluginContext):
    | void
    | (() => void)
    | PluginInstance;
}
