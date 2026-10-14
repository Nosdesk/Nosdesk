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
}

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
  return { released: false } as OwnerState;
}

/**
 * Open a dialog for `owner` and wait for the user's answer. Throws `invalid`
 * when the owner was released (the plugin instance is no longer mounted).
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
  entry.settle(value === undefined ? CANCELLED[entry.request.kind] : value);
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
