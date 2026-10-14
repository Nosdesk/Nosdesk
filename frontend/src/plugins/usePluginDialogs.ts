/**
 * Host-drawn plugin dialogs (`api.ui.confirm` and the `api.ui.pick*` calls).
 *
 * A sandboxed plugin's iframe is sized to its content, so anything it draws as
 * an overlay is clipped to the panel. These calls ask the host to draw the
 * app's own dialog instead and hand back what the user chose.
 *
 * Module state, a stack: `PluginDialogHost` (mounted once at app root) renders
 * every pending request, later ones over earlier ones, and over an open plugin
 * modal. Unlike `usePluginModal` (one slot, replaced on open) nothing here is
 * replaced: each request holds a resolver the plugin is awaiting, so each must
 * settle exactly once.
 *
 * Requests belong to an owner, one per live plugin API instance. When that
 * instance goes away (its frame unmounts or reloads) `releaseDialogOwner`
 * closes its dialogs and settles them as cancelled; a released owner can't open
 * new ones.
 *
 * An owner that keeps reopening dialogs the user dismisses would trap them, so
 * three dismissals in a row within 30 seconds block its dialogs for 60 seconds
 * (or until the plugin remounts, which brings a new owner). A completed pick or
 * a confirmed dialog resets the count; dialogs closed by an unmount don't count.
 */
import { shallowRef, computed, type ComputedRef } from 'vue';
import { PluginApiError } from '@nosdesk/plugin-sdk';
import type {
  PluginConfirmOptions,
  PluginPickedAsset,
  PluginPickedTicket,
  PluginPickedUser,
} from '@nosdesk/plugin-sdk';

/** What each dialog kind resolves to. Cancel is `false` for confirm, `null`
 * for every picker. */
export interface PluginDialogResults {
  confirm: boolean;
  user: PluginPickedUser | null;
  asset: PluginPickedAsset | null;
  ticket: PluginPickedTicket | null;
  date: string | null;
}

export type PluginDialogKind = keyof PluginDialogResults;

interface DialogOptionsByKind {
  confirm: PluginConfirmOptions;
  user: { title?: string };
  asset: { title?: string };
  ticket: { title?: string };
  date: { title?: string; initial?: string };
}

/** A pending dialog as the host renders it. */
export type PluginDialogRequest = {
  [K in PluginDialogKind]: {
    id: number;
    kind: K;
    /** The plugin's display name, for the attribution every dialog carries. */
    pluginName: string;
    options: DialogOptionsByKind[K];
  };
}[PluginDialogKind];

/** One live plugin API instance's handle on the dialog stack. */
export interface PluginDialogOwner {
  readonly released: boolean;
}

interface OwnerState extends PluginDialogOwner {
  released: boolean;
  /** When the user dismissed this owner's latest dialogs, in a row. */
  dismissals: number[];
  /** Dialogs are refused until this time (ms since epoch), 0 when not blocked. */
  blockedUntil: number;
}

/** Consecutive dismissals that block an owner's dialogs. */
const DISMISSAL_LIMIT = 3;
/** The dismissals must fall within this window to count as a loop. */
const DISMISSAL_WINDOW_MS = 30_000;
/** How long a blocked owner's dialogs are refused. */
const DISMISSAL_BLOCK_MS = 60_000;

interface Entry {
  request: PluginDialogRequest;
  owner: OwnerState;
  settle: (value: unknown) => void;
}

const CANCELLED: { [K in PluginDialogKind]: PluginDialogResults[K] } = {
  confirm: false,
  user: null,
  asset: null,
  ticket: null,
  date: null,
};

const entries = shallowRef<Entry[]>([]);
let nextId = 1;

/** Pending dialogs, oldest first (render order: later ones stack on top). */
export const pluginDialogs: ComputedRef<PluginDialogRequest[]> = computed(() =>
  entries.value.map((e) => e.request),
);

/** A fresh owner for one plugin API instance. */
export function createDialogOwner(): PluginDialogOwner {
  const state: OwnerState = { released: false, dismissals: [], blockedUntil: 0 };
  return state;
}

/**
 * Open a dialog for `owner` and wait for the user's answer. Throws `invalid`
 * when the owner was released (the plugin instance is no longer mounted), and
 * `denied` while the owner is blocked for repeated dismissals.
 */
export function openPluginDialog<K extends PluginDialogKind>(
  owner: PluginDialogOwner,
  kind: K,
  pluginName: string,
  options: DialogOptionsByKind[K],
): Promise<PluginDialogResults[K]> {
  const state = owner as OwnerState;
  if (state.released) {
    return Promise.reject(
      new PluginApiError('invalid', 'this plugin instance is no longer mounted, so it cannot open a dialog'),
    );
  }
  if (Date.now() < state.blockedUntil) {
    return Promise.reject(
      new PluginApiError('denied', "the user dismissed this plugin's dialogs repeatedly"),
    );
  }
  return new Promise<PluginDialogResults[K]>((resolve) => {
    const request = { id: nextId++, kind, pluginName, options } as PluginDialogRequest;
    const entry: Entry = {
      request,
      owner: state,
      settle: (value) => resolve(value as PluginDialogResults[K]),
    };
    entries.value = [...entries.value, entry];
  });
}

/**
 * Settle a pending dialog with the user's answer (or `undefined` for
 * cancelled) and close it. Idempotent: the reused pickers emit their pick and
 * then `close`, and only the first settles.
 */
export function settlePluginDialog(id: number, value?: unknown): void {
  const entry = entries.value.find((e) => e.request.id === id);
  if (!entry) return;
  entries.value = entries.value.filter((e) => e !== entry);
  const result = value === undefined ? CANCELLED[entry.request.kind] : value;
  recordAnswer(entry.owner, result === CANCELLED[entry.request.kind]);
  entry.settle(result);
}

/** Count a user dismissal toward the owner's block, or reset on an answer. */
function recordAnswer(owner: OwnerState, dismissed: boolean): void {
  if (!dismissed) {
    owner.dismissals = [];
    return;
  }
  const now = Date.now();
  owner.dismissals = [...owner.dismissals.filter((t) => now - t < DISMISSAL_WINDOW_MS), now];
  if (owner.dismissals.length >= DISMISSAL_LIMIT) {
    owner.blockedUntil = now + DISMISSAL_BLOCK_MS;
    owner.dismissals = [];
  }
}

/**
 * The owner's plugin instance is gone: close its open dialogs, settle them as
 * cancelled, and refuse any it tries to open from now on.
 */
export function releaseDialogOwner(owner: PluginDialogOwner): void {
  const state = owner as OwnerState;
  state.released = true;
  const mine = entries.value.filter((e) => e.owner === state);
  if (mine.length === 0) return;
  entries.value = entries.value.filter((e) => e.owner !== state);
  for (const e of mine) e.settle(CANCELLED[e.request.kind]);
}
