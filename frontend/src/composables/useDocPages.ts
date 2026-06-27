/**
 * Pool-native documentation reads, adapted to the `Page` DTO the
 * existing documentation components (cards, grids, tree items) already
 * render. Lists derive from the sync object pool, so bootstrap and
 * live SSE / delta updates flow through automatically — no REST tree
 * loads and no discrete `documentation-updated` listeners.
 *
 * `Page.created_by` / `last_edited_by` are resolved from the `user`
 * pool aggregate (the bootstrap ships the full roster), so the leaf
 * components keep rendering author name + avatar unchanged. The Yjs
 * body (`content`) is intentionally empty here: it flows through the
 * collaborative-editor channel, and these list/grid surfaces never
 * showed it (the old REST mappers passed `content: ''` too).
 */
import { computed, toValue, type ComputedRef, type MaybeRefOrGetter } from 'vue'
import * as pool from '@nosdesk/core/sync/pool'
import {
  useSyncDocsStore,
  buildPageTree,
  isActivePage,
  type DocPageRow,
  type DocPageNode,
} from '@nosdesk/core/sync/stores/documentation'
import type { Page } from '@nosdesk/core/services/documentationService'
import type { UserInfo, PlatformRole } from '@nosdesk/core/types/user'
import type { WorkspaceRole } from '@nosdesk/core/types/workspace'

interface PoolUser {
  uuid: string
  name: string
  email: string
  platform_role: PlatformRole
  workspace_role?: WorkspaceRole | null
  avatar_url?: string | null
  avatar_thumb?: string | null
}

/** Resolve a user uuid against the user pool into a `UserInfo`. */
function resolveUser(uuid: string | null | undefined): UserInfo | undefined {
  if (!uuid) return undefined
  const u = pool.get<PoolUser>('user', uuid)
  if (!u) return undefined
  return {
    uuid: u.uuid,
    name: u.name,
    email: u.email,
    platform_role: u.platform_role,
    workspace_role: u.workspace_role ?? null,
    avatar_url: u.avatar_url ?? null,
    avatar_thumb: u.avatar_thumb ?? null,
  }
}

/** Adapt a pool page row (+ optional children) to the `Page` DTO. */
export function toPage(row: DocPageRow, children: Page[] = []): Page {
  return {
    id: row.id,
    uuid: row.uuid,
    slug: row.slug,
    title: row.title,
    description: null,
    content: '',
    parent_id: row.parent_id,
    author: '',
    status: row.status,
    icon: row.icon,
    children,
    display_order: row.display_order ?? undefined,
    created_at: row.created_at,
    updated_at: row.updated_at,
    lastUpdated: row.updated_at,
    archived_at: row.archived_at,
    deleted_at: row.deleted_at,
    created_by: resolveUser(row.created_by),
    last_edited_by: resolveUser(row.last_edited_by),
    verified_by: resolveUser(row.verified_by) ?? null,
    verified_at: row.verified_at,
    verify_interval_days: row.verify_interval_days,
  }
}

/** Adapt a page tree node recursively. */
export function nodeToPage(node: DocPageNode): Page {
  return toPage(node, node.children.map(nodeToPage))
}

/**
 * Flat, card-ready `Page[]` views over the pool. Each is a computed
 * that re-derives whenever the documentation or user pool changes.
 */
export function useDocPages() {
  const docs = useSyncDocsStore()

  const drafts = computed<Page[]>(() =>
    docs.allPages
      .filter((p) => p.collection_id == null && isActivePage(p))
      .map((p) => toPage(p)),
  )

  const archived = computed<Page[]>(() =>
    docs.allPages.filter((p) => p.status === 'archived').map((p) => toPage(p)),
  )

  const trashed = computed<Page[]>(() =>
    docs.allPages.filter((p) => p.status === 'deleted').map((p) => toPage(p)),
  )

  // The full active documentation tree (parent_id hierarchy across all
  // collections) — what the index "browse all" / recently-updated use.
  const allTree = computed<Page[]>(() =>
    buildPageTree(docs.allPages.filter(isActivePage)).map(nodeToPage),
  )

  /** Active page tree for a collection, as `Page` nodes. */
  function collectionTree(collectionId: MaybeRefOrGetter<number | null>): ComputedRef<Page[]> {
    return computed(() => {
      const cid = toValue(collectionId)
      if (cid == null) return []
      const rows = docs.allPages.filter((p) => p.collection_id === cid && isActivePage(p))
      return buildPageTree(rows).map(nodeToPage)
    })
  }

  return { drafts, archived, trashed, allTree, collectionTree }
}
