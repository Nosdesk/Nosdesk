// Shared partition of an asset kind's attribute schema into the
// user-editable properties and the Microsoft Graph (Intune / Entra)
// sync-owned ones. Mirrors the backend `SYNC_OWNED_ATTRIBUTE_KEYS` in
// services/assets/mod.rs. Sync-owned keys are written by the sync, never
// typed by a human, so they render read-only on a synced asset and are
// never offered as a model's default specs.

export const SYNC_OWNED_ATTRIBUTE_KEYS: ReadonlySet<string> = new Set([
  'hostname',
  'is_managed',
  'os_version',
  'operating_system',
  'last_sync_time',
  'enrollment_date',
  'entra_device_id',
  'compliance_state',
  'intune_device_id',
  'microsoft_device_id',
]);

export function isSyncOwnedKey(key: string): boolean {
  return SYNC_OWNED_ATTRIBUTE_KEYS.has(key);
}

/** Build a schema containing only the properties whose key passes
 *  `pred`, or null when none match (so callers can `v-if` cleanly). */
export function partitionKindSchema(
  schema: Record<string, unknown> | null | undefined,
  pred: (key: string) => boolean,
): Record<string, unknown> | null {
  if (!schema) return null;
  const props = (schema.properties as Record<string, unknown>) ?? {};
  const filtered: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(props)) {
    if (pred(key)) filtered[key] = value;
  }
  if (Object.keys(filtered).length === 0) return null;
  return { ...schema, properties: filtered };
}

/** The user-editable slice of a kind's schema. This is the surface a
 *  model's default specs are authored against. */
export function userAttributeSchema(
  schema: Record<string, unknown> | null | undefined,
): Record<string, unknown> | null {
  return partitionKindSchema(schema, (k) => !SYNC_OWNED_ATTRIBUTE_KEYS.has(k));
}

/** The sync-owned slice of a kind's schema (read-only on a synced asset). */
export function syncAttributeSchema(
  schema: Record<string, unknown> | null | undefined,
): Record<string, unknown> | null {
  return partitionKindSchema(schema, (k) => SYNC_OWNED_ATTRIBUTE_KEYS.has(k));
}
