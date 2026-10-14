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
  User,
  CollectionRow,
  CollectionListResponse,
  PluginEvent,
} from '@nosdesk/core/types';

export type { Ticket, Asset, User, CollectionRow, CollectionListResponse, PluginEvent };

/** A comment a plugin adds to a ticket. */
export interface PluginComment {
  content: string;
  is_internal?: boolean;
}

/** The writable-field subset a plugin may patch on a ticket (`ticket:write`).
 * Deliberately narrow, not the full ticket: a patch with any other key is
 * refused with an `invalid` PluginApiError naming the key. */
export interface PluginTicketPatch {
  title?: string;
  priority?: string;
  workflow_state_id?: number;
  assignee?: string | null;
}

/** The writable-field subset a plugin may patch on an asset (`asset:write`).
 * A patch with any other key is refused with an `invalid` PluginApiError
 * naming the key. */
export interface PluginAssetPatch {
  name?: string;
  /**
   * @deprecated Not writable by plugins. Status changes go through the asset
   * lifecycle, which plugins can't use yet; a patch carrying `status` is
   * refused with an `invalid` PluginApiError.
   */
  status?: string;
  location?: string | null;
}

/** Filters and paging for `assets.query` (`asset:read`). List-valued filters
 * match ANY of the given values. */
export interface PluginAssetQuery {
  /** 1-based page (default 1). */
  page?: number;
  /** Rows per page, 1 to 100 (default 25). */
  pageSize?: number;
  /** Matches name, serial number, model, manufacturer, or an exact asset id. */
  search?: string;
  /** Asset kind slug(s), as `assets.kinds()` lists them. */
  kind?: string | string[];
  /** Native asset group id(s). */
  group?: number | number[];
  /** Lifecycle status(es): `in_service`, `in_repair`, and so on. */
  status?: string | string[];
  /** A column (`name`, `serial_number`, `model`, `location`, `status`,
   * `created_at`, `updated_at`, `quantity`), `borrower` (open loan's
   * borrower name), `last_status_change` (newest lifecycle event) or
   * `attr:<key>` for a kind attribute. Default `name`. */
  sortBy?: string;
  sortOrder?: 'asc' | 'desc';
}

/** One page of `assets.query`. Each row also carries `current_loan` (the
 * open loan: borrower uuid and name, `issued_at`, `due_at`) and
 * `latest_lifecycle` (the newest status change: `from_status`, `to_status`,
 * `occurred_at`, and for staff `reason` and `actor_*`), each null when there
 * is none. */
export interface PluginAssetPage {
  assets: Asset[];
  total: number;
  page: number;
  pageSize: number;
  totalPages: number;
}

/** An asset kind and the attributes its assets carry (`asset:read`).
 * `attributeSchema` is a JSON Schema subset: `{ type: 'object', properties,
 * required? }`, each property a primitive or an array of primitives, with
 * `format` among `date`, `date-time`, `email`, `uri`, `user-uuid`, `asset-ref`. */
export interface PluginAssetKind {
  id: number;
  slug: string;
  label: string;
  description: string | null;
  icon: string | null;
  /** `it` | `logical` | `physical` | `bulk` | `generic`. */
  category: string;
  attributeSchema: Record<string, unknown>;
  sortOrder: number;
  isBuiltin: boolean;
}

/** A native asset group, as `assets.groups()` lists it. `id` is the value
 * `assets.query({ group })` filters on. */
export interface PluginAssetGroup {
  id: number;
  name: string;
  /** The group's colour as the app stores it (a CSS colour), or null. */
  color: string | null;
}

/** A plugin storage entry with its version, for `storage.compareAndSet`.
 * `version` is 0 when the key isn't set, and goes up by one on every write. */
export interface PluginStorageEntry<T = unknown> {
  value: T | null;
  version: number;
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
 * `timeout` | `conflict` | `upstream`). `null` is returned only for a genuine not-found, a
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
    /** Every asset in one unpaginated call. Prefer `query` for large
     * inventories; `list` stays for compatibility. */
    list(): Promise<Asset[]>;
    /** One page of assets, filtered by search, kind, group and status
     * (`asset:read`). */
    query(params?: PluginAssetQuery): Promise<PluginAssetPage>;
    /** The workspace's asset kinds with their attribute schemas
     * (`asset:read`). Reading kinds also needs the acting user to be an agent
     * or admin; for anyone else the call throws `upstream`. */
    kinds(): Promise<PluginAssetKind[]>;
    /** The workspace's active (not archived) asset groups (`asset:read`),
     * since API 1.2. A user who isn't an agent or admin gets an empty list
     * unless the workspace shows assets to requesters. */
    groups(): Promise<PluginAssetGroup[]>;
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
    /** The value with its version, for a later `compareAndSet`. */
    getEntry<T>(key: string): Promise<PluginStorageEntry<T>>;
    /** Unconditional write (last writer wins). */
    set<T>(key: string, value: T): Promise<boolean>;
    /** Write only if the entry is still at `expectedVersion` (0 = the key must
     * not exist yet). Resolves to the new version; throws a `conflict`
     * PluginApiError when someone else wrote first, so read again, merge, and
     * retry. */
    compareAndSet<T>(key: string, value: T, expectedVersion: number): Promise<number>;
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
  /**
   * UI the host draws natively, over the whole app instead of inside the
   * plugin's iframe, so it is never clipped to the panel. There is no
   * `ui.toast`: `notify` already is the toast.
   *
   * Everything here except `isPrinting` arrived in runtime API 1.1. Feature
   * detect before calling on an older host:
   *
   *     const [major, minor] = (await api.version).split('.').map(Number);
   *     const hasHostDialogs = major > 1 || (major === 1 && minor >= 1);
   *
   * Dialogs (`confirm` and the `pick*` methods) wait on the user, so they never
   * time out, and a plugin instance may have only one open at a time: a second
   * call while one is pending throws `rate_limited`. Every dialog names the
   * plugin, so the user can tell it apart from the app's own. If the plugin is
   * unmounted while a dialog is open (the user navigates away, the panel
   * closes), the dialog closes and resolves as cancelled. Don't reopen a dialog
   * the user dismissed: after three dismissals in a row within 30 seconds, the
   * instance's dialogs throw `denied` for 60 seconds or until it remounts. A
   * pick or a confirm resets the count.
   *
   * The pickers need no permission: the user chooses what to share, and the
   * result is a minimal projection. Fetch more of the record through the
   * permission-gated `get` methods. A picker lists records with the viewer's own
   * session, so it shows only what the viewer may see; a viewer who can't list
   * users, assets or tickets gets an empty or refused list.
   */
  ui: PluginUiApi;
}

/** The host-rendered UI calls on `HostApi.ui`. */
export interface PluginUiApi {
  /** Whether the page is being printed. */
  isPrinting(): Promise<boolean>;
  /**
   * Ask the user to confirm an action, in the app's own confirmation dialog.
   * Resolves `true` on confirm and `false` on cancel, Escape, or the plugin
   * unmounting. `danger` styles the confirm button as destructive. Since 1.1.
   */
  confirm(opts: PluginConfirmOptions): Promise<boolean>;
  /**
   * Let the user choose a workspace member. Resolves to `{ uuid, name }` (no
   * email; read more with `users.get`, which needs `user:read`), or `null` on
   * cancel. Lists people with the viewer's session: a viewer who can't list
   * users sees an empty or refused picker. No permission needed. Since 1.1.
   */
  pickUser(opts?: PluginPickOptions): Promise<PluginPickedUser | null>;
  /**
   * Let the user choose an asset. Resolves to `{ id, name, assetTag }`, or
   * `null` on cancel. Lists assets with the viewer's session: a viewer who
   * can't list assets sees an empty or refused picker. Read the full asset with
   * `assets.get` (`asset:read`). No permission needed. Since 1.1.
   */
  pickAsset(opts?: PluginPickOptions): Promise<PluginPickedAsset | null>;
  /**
   * Let the user choose a ticket. Resolves to `{ id, title }` (the ticket id,
   * not its number), or `null` on cancel. Lists tickets with the viewer's
   * session: a viewer who can't list tickets sees an empty or refused picker.
   * Read the full ticket with `tickets.get` (`ticket:read`). No permission
   * needed. Since 1.1.
   */
  pickTicket(opts?: PluginPickOptions): Promise<PluginPickedTicket | null>;
  /**
   * Let the user choose a date. Resolves to a date-only ISO string
   * (`YYYY-MM-DD`), or `null` on cancel. `initial` preselects a date in the
   * same format; anything else throws `invalid`. Since 1.1.
   */
  pickDate(opts?: PluginPickDateOptions): Promise<string | null>;
  /**
   * Open a ticket, asset or user in the app. `id` is the ticket id, the asset
   * id, or the user's uuid. Throws `invalid` for a malformed reference.
   * Resolves once navigation starts; the new page may unmount the plugin. The
   * page itself checks the viewer's access as usual. Since 1.1.
   */
  openRecord(ref: PluginRecordRef): Promise<void>;
  /**
   * Open one of this plugin's own full-page components (a `nav.item` or
   * `nav.assets.item` in its manifest), by its `components` key. Any other name
   * throws `invalid`. Resolves once navigation starts. Since 1.1.
   */
  openPage(componentName: string): Promise<void>;
}

/** Options for `ui.confirm`. */
export interface PluginConfirmOptions {
  /** The question, shown as the dialog title. */
  title: string;
  /** Optional detail under the title. */
  message?: string;
  /** Confirm button label (default: the app's "Confirm"). */
  confirmLabel?: string;
  /** Cancel button label (default: the app's "Cancel"). */
  cancelLabel?: string;
  /** Style the confirm button as destructive. */
  danger?: boolean;
}

/** Options for the `ui.pick*` methods. */
export interface PluginPickOptions {
  /** Dialog title. The plugin's name is shown with it either way. */
  title?: string;
}

/** Options for `ui.pickDate`. */
export interface PluginPickDateOptions extends PluginPickOptions {
  /** Preselected date, `YYYY-MM-DD`. */
  initial?: string;
}

/** The user `ui.pickUser` resolves to. */
export interface PluginPickedUser {
  uuid: string;
  name: string;
}

/** The asset `ui.pickAsset` resolves to. */
export interface PluginPickedAsset {
  id: number;
  name: string;
  assetTag: string | null;
}

/** The ticket `ui.pickTicket` resolves to. */
export interface PluginPickedTicket {
  id: number;
  title: string;
}

/** A record `ui.openRecord` opens: a ticket or asset by its numeric id, a user
 * by uuid. */
export interface PluginRecordRef {
  type: 'ticket' | 'asset' | 'user';
  id: number | string;
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

/** A single structured address a plugin enriches (`user.address.panel`). A clean
 * camelCase projection of the host's vCard address row, plus `formatted` (the
 * fields joined into one line) for geocoding / display. */
export interface PluginAddress {
  id: number;
  /** `work` / `home` / `other`. */
  addressType: string;
  isPrimary: boolean;
  street: string | null;
  city: string | null;
  region: string | null;
  postalCode: string | null;
  country: string | null;
  label: string | null;
  /** The non-empty fields joined with ", " — the string to geocode / show. */
  formatted: string;
}

/** The host-provided context, snapshotted into the iframe. It is not a live
 * object (it can't cross postMessage reactively); updates arrive via
 * `onContextChange`. */
/**
 * Host layout facts a plugin cannot derive from inside its own iframe.
 *
 * A sandboxed panel is a separate document, so its media queries resolve
 * against the IFRAME box, not the app viewport. That is the right default for
 * laying a panel out (see `data-nd-container` in the runtime), but it means a
 * plugin has no way to see app-level decisions: a 336px sidebar panel measures
 * the same on a phone as on a 4K display, while the app around it has switched
 * layout entirely.
 *
 * Deliberately only what the guest CANNOT work out for itself. Pointer type,
 * colour scheme and reduced-motion are device-level and resolve correctly
 * inside the iframe, so they stay guest-side rather than being mirrored here;
 * the app's viewport width is the one fact that does not survive the boundary.
 *
 * The value is BUCKETED, never raw pixels, so the host pushes only when the
 * bucket actually flips rather than on every resize. A plugin that wants its
 * own width should measure itself or read `--nd-container-width`.
 */
export interface PluginLayout {
  /** The app's active viewport breakpoint, on the host's own scale
   *  (`sm` 640, `md` 768, `lg` 1024, `xl` 1280; `base` below `sm`). Use this to
   *  MATCH an app-level decision, not to lay out the panel itself. */
  breakpoint: 'base' | 'sm' | 'md' | 'lg' | 'xl';
}

export interface PluginContext {
  ticket: Ticket | null;
  asset: Asset | null;
  /** The profile being viewed (`user.sidebar.panel`). Null outside a user
   * context. */
  user: User | null;
  /** The address this panel enriches (`user.address.panel`). Null elsewhere. */
  address: PluginAddress | null;
  /** Selected ticket ids for a bulk action (`ticket.bulk.action`). The plugin
   * operates on the whole selection. Null / absent outside a bulk context. */
  ticketIds?: number[] | null;
  /** Which of the plugin's manifest components this mount is rendering. A bundle
   * may declare several (different slots); the plugin switches its UI on `name`
   * (the manifest `components` map key) / `slot`. */
  component: { name: string; slot: string };
  /** Host layout signals; see `PluginLayout`. Absent on an older host. */
  layout?: PluginLayout;
  /** Monotonic counter for this component's declared `action` (a host menu
   * trigger). Increments on each activation; absent if the component declares no
   * action. The plugin reacts (e.g. opens a panel) when it changes. */
  actionActivated?: number;
}

/** The host's active design language, snapshotted into the iframe so a plugin's
 * UI matches the app. The runtime injects `tokens` as `--nd-*` CSS variables and
 * a base stylesheet, so most plugins never touch this object; it is exposed for
 * the rare plugin that must branch in JS (e.g. light vs dark map tiles). Values
 * are resolved for whatever theme is active, and re-pushed when it changes. */
export interface PluginTheme {
  /** Resolved `--nd-*` token values, keyed WITHOUT the `--nd-` prefix
   * (`surface`, `text`, `accent`, ...). Injected as CSS variables by the runtime. */
  tokens: Record<string, string>;
  /** Coarse light/dark signal, for plugins that pick assets by scheme. */
  colorScheme: 'light' | 'dark';
  /** The active theme id (`light`, `dark`, `epaper`, ...). */
  name: string;
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
