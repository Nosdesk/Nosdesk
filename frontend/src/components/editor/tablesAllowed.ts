/**
 * Which collaborative documents may gain tables in this app run.
 *
 * A document's server says so in its hello once it knows tables (schema 2).
 * Until then this client creates none: a server that predates tables still
 * serves clients that would delete one. The permission lasts for the rest of
 * the app run, so a dropped connection doesn't take it away mid-edit.
 */
import { shallowReactive } from 'vue'

const allowed = shallowReactive(new Set<string>())

/** Whether this client may add tables to `docId`. False until allowed. */
export function tablesAllowed(docId: string): boolean {
  return allowed.has(docId)
}

/** Let this client add tables to `docId`, for the rest of the app run. */
export function allowTables(docId: string): void {
  allowed.add(docId)
}

/** Tests only: forget every permission. */
export function resetTablesAllowed(): void {
  allowed.clear()
}
