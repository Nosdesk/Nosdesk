import type { Plugin } from '@nosdesk/core/types/plugin';

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
