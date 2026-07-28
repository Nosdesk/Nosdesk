/**
 * On-demand plugin modal surface.
 *
 * A host-invoked, transient surface: an action contribution (e.g. a
 * `ticket.header.action` menu item) opens a plugin's component in a modal that
 * floats over the page, rather than poking an already-mounted panel. This is
 * the "modal" reserved surface from the UI-slots design (the Zendesk/Slack
 * pattern) and is what makes the action mechanism usable without the plugin
 * also owning a persistent panel.
 *
 * Module state, so any action handler can open the single shared modal that
 * `PluginModalHost` (mounted once at app root) renders. Opening a second one
 * replaces the first — one plugin modal at a time.
 */
import { shallowRef, computed, type ComputedRef } from 'vue';
import type { PluginSlotContext } from './context';

export interface PluginModalRequest {
  pluginUuid: string;
  /** Manifest component key to render (its `slot` is the invoking action slot). */
  componentName: string;
  /** Slot the component was contributed to (passed through to the frame). */
  slot: string;
  /** Host context handed to the component (e.g. the ticket the action fired on). */
  context?: PluginSlotContext;
  /** Optional explicit title; falls back to the component label / plugin name. */
  title?: string;
}

const current = shallowRef<PluginModalRequest | null>(null);

/** The open modal request, or null when closed. Mutate only via open/close. */
export const pluginModal: ComputedRef<PluginModalRequest | null> = computed(() => current.value);

/** Open (or replace) the plugin modal. */
export function openPluginModal(request: PluginModalRequest): void {
  current.value = request;
}

/** Close the plugin modal if open. */
export function closePluginModal(): void {
  current.value = null;
}
