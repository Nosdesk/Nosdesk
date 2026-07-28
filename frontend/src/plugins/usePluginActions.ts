/**
 * Plugin action contributions (mechanism B: host-rendered chrome).
 *
 * A plugin panel component may declare an `action` (a host menu trigger). The
 * host renders it as a native menu item; selecting it bumps a per-entity
 * activation counter that flows to the plugin's sandbox frame as
 * `context.actionActivated`, and the plugin reacts (e.g. opens its panel).
 *
 * This module is the SINGLE place the action id / activation-key format lives.
 * Previously the `plugin:<uuid>:<component>` string was built in the ticket
 * overflow menu, parsed in its select handler, and rebuilt in PluginSlot to
 * look up the counter, with the counter store living in core's `ticketUi`.
 * Everything is consolidated here and generalized to any entity scope, so a new
 * host surface (asset menu, bulk bar) wires actions the same way.
 */
import { computed, reactive, unref, type ComputedRef, type MaybeRef } from 'vue';
import { getActionRegistrations } from './loader';
import type { PluginSlot } from '@nosdesk/core/types/plugin';

// Namespaces a plugin action id so it coexists with native menu items.
const MENU_PREFIX = 'plugin:';

/** Menu-item id for a plugin action. Pass the selected id back to `activate()`. */
export function pluginMenuActionId(pluginUuid: string, componentName: string): string {
  return `${MENU_PREFIX}${pluginUuid}:${componentName}`;
}

/**
 * The key a PluginSlot uses to look up a component's activation counter. Equal
 * to the menu id with the namespace stripped, so the two always agree.
 */
export function pluginActivationKey(pluginUuid: string, componentName: string): string {
  return `${pluginUuid}:${componentName}`;
}

function menuIdToActivationKey(menuId: string): string | undefined {
  return menuId.startsWith(MENU_PREFIX) ? menuId.slice(MENU_PREFIX.length) : undefined;
}

/** Parse a plugin menu-action id back into its plugin + component parts. */
export function parsePluginMenuActionId(
  menuId: string,
): { pluginUuid: string; componentName: string } | undefined {
  const key = menuIdToActivationKey(menuId);
  if (key === undefined) return undefined;
  const sep = key.indexOf(':');
  if (sep <= 0 || sep === key.length - 1) return undefined;
  return { pluginUuid: key.slice(0, sep), componentName: key.slice(sep + 1) };
}

/** Stable scope string isolating one entity's activation counters. */
export function pluginActionScope(entity: string, id: string | number): string {
  return `${entity}:${id}`;
}

// scope -> (activationKey -> monotonic counter). Module state: survives
// component unmount (nav-away/back within a tab). Callers clear a scope when the
// entity is gone (e.g. a deleted ticket) via `clearPluginActionScope`.
const activations = reactive(new Map<string, Map<string, number>>());

// Shared, stable empty map for scopes with no activations yet — a fresh literal
// each read would defeat the computed's memoization.
const EMPTY: ReadonlyMap<string, number> = new Map();

/** Drop every activation counter for a scope. */
export function clearPluginActionScope(scope: string): void {
  activations.delete(scope);
}

export interface PluginActionItem {
  /** Namespaced menu-item id; pass back to `activate()` on select. */
  id: string;
  label: string;
  icon?: string;
  trailing?: string;
}

export function usePluginActions(target: PluginSlot, scope: MaybeRef<string | undefined>) {
  const items: ComputedRef<PluginActionItem[]> = computed(() =>
    getActionRegistrations(target).map((a) => ({
      id: pluginMenuActionId(a.pluginUuid, a.componentName),
      label: a.label,
      icon: a.icon,
      trailing: a.componentLabel || a.pluginName,
    })),
  );

  const activatedMap: ComputedRef<ReadonlyMap<string, number>> = computed(() => {
    const s = unref(scope);
    return (s !== undefined ? activations.get(s) : undefined) ?? EMPTY;
  });

  /**
   * Bump the activation counter for a menu id. A no-op for non-plugin ids and
   * when the scope is unset, so a shared select handler can call it with any
   * menu id.
   */
  function activate(menuId: string): void {
    const s = unref(scope);
    if (s === undefined) return;
    const key = menuIdToActivationKey(menuId);
    if (key === undefined) return;
    let map = activations.get(s);
    if (!map) {
      activations.set(s, new Map());
      map = activations.get(s)!;
    }
    map.set(key, (map.get(key) ?? 0) + 1);
  }

  return { items, activatedMap, activate };
}
