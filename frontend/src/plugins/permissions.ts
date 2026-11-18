import { PLUGIN_EVENTS, type Plugin } from '@nosdesk/core/types/plugin';

/**
 * A plugin's EFFECTIVE permission grant: the CONSENTED set, falling back to the
 * manifest only for legacy rows with no consent recorded
 * (`consented_permissions === null`). An admin may have approved a narrower
 * scope than the manifest requests, and the manifest can widen on update ahead
 * of re-consent, so the consented set is the authority.
 *
 * Mirrors the backend's `Plugin::effective_permission_set`. This is the single
 * fail-closed definition the host API boundary (`api.ts`) and the event
 * dispatcher (`eventDispatcher.ts`) both gate on, so the rule can't drift.
 */
export function effectivePermissions(plugin: Plugin): string[] {
  return plugin.consented_permissions ?? plugin.manifest.permissions;
}

/**
 * Whether a plugin's manifest lists the host events it listens to. Without
 * the list, `api.on` accepts any known host event, as before the field
 * existed, and the plugin frame warns that the next plugin API major version
 * requires it.
 */
export function subscriptionsDeclared(plugin: Plugin): boolean {
  return Array.isArray(plugin.manifest.subscriptions);
}

/**
 * The host events a plugin may listen to: its manifest `subscriptions`, or
 * any known host event when the manifest has no such list. (`events` is the
 * plugin's own events, which it emits.) `api.on` and the event dispatcher
 * both gate on this.
 */
export function pluginSubscriptions(plugin: Plugin): readonly string[] {
  return plugin.manifest.subscriptions ?? PLUGIN_EVENTS;
}
