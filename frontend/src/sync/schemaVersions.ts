import type { SyncAggregate } from '@nosdesk/core/sync/types'

/**
 * Schema versions per aggregate, mirroring `sync::registry` on the
 * server. Rows in IndexedDB whose recorded schema_version doesn't
 * match the entry here get filtered on rehydrate and re-fetched on
 * the next bootstrap.
 *
 * Only aggregates listed here are cached. The live-only ones
 * (`assignment`, `group_membership`, `plugin`, `asset_media`,
 * `asset_lifecycle_event`) are left out on purpose: no snapshot carries
 * them and no view reads them from the pool, so caching them only grew
 * the database.
 *
 * When the server's manifest schema_version bumps, bump the matching
 * entry here in the same commit.
 */
export const SCHEMA_VERSIONS: Partial<Record<SyncAggregate, number>> = {
  ticket: 2,
  project: 1,
  project_ticket: 1,
  ticket_asset: 1,
  linked_ticket: 1,
  workflow_state: 1,
  comment: 1,
  attachment: 2,
  user: 2,
  asset: 1,
  cycle: 1,
  documentation_page: 1,
  documentation_collection: 2,
}

/**
 * Tag identifying the client aggregate versions the cache was written under.
 * Compared in partitionWarmGroups: rows persisted under a different map may
 * have been filtered out on rehydrate, and only a full bootstrap re-fetches
 * them, so any mismatch disqualifies the warm path wholesale.
 */
export const MODEL_VERSIONS_TAG = JSON.stringify(SCHEMA_VERSIONS)
