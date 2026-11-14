import type { SyncAggregate } from '@nosdesk/core/sync/types'

/**
 * Schema versions per aggregate, mirroring `sync::registry` on the
 * server. Rows in IndexedDB whose recorded schema_version doesn't
 * match the entry here get filtered on rehydrate and re-fetched on
 * the next bootstrap.
 *
 * When the server's manifest schema_version bumps, bump the matching
 * entry here in the same commit.
 */
export const SCHEMA_VERSIONS: Partial<Record<SyncAggregate, number>> = {
  ticket: 1,
  project: 1,
  project_ticket: 1,
  ticket_asset: 1,
  linked_ticket: 1,
  workflow_state: 1,
  comment: 1,
  attachment: 2,
  assignment: 1,
  group_membership: 1,
  plugin: 1,
  user: 2,
  asset: 1,
  asset_media: 2,
  asset_lifecycle_event: 1,
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
