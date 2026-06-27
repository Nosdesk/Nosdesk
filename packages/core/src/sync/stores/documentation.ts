/**
 * Documentation sync facade.
 *
 * Pool-derived reads for the `documentation_page` and
 * `documentation_collection` aggregates. Pages carry a denormalised
 * `collection_id` (one collection per page, enforced by the backend's
 * UNIQUE(page_id)) so collection grouping is a plain filter, and
 * `parent_id` gives the in-collection hierarchy.
 *
 * No reactive state of its own — everything derives from the object
 * pool, so bootstrap snapshots and live SSE / delta updates flow
 * through automatically (mirrors the projects / tickets sync stores).
 * Mutations still go through the REST documentationService /
 * collectionService; only reads move onto the pool.
 */
import { defineStore } from 'pinia'
import { computed, toValue, type ComputedRef, type MaybeRefOrGetter } from 'vue'
import { useEntity, useAggregate } from '../composables'

/** A documentation_page sync row — mirrors `page_sync_payload`. */
export interface DocPageRow {
  id: number
  uuid: string
  collection_id: number | null
  title: string
  slug: string
  icon: string | null
  cover_image: string | null
  status: string
  parent_id: number | null
  display_order: number | null
  is_public: boolean
  is_template: boolean
  archived_at: string | null
  deleted_at: string | null
  created_by: string
  last_edited_by: string
  verified_by: string | null
  verified_at: string | null
  verify_interval_days: number | null
  created_at: string
  updated_at: string
}

/** A documentation_collection sync row — mirrors `collection_sync_payload`. */
export interface DocCollectionRow {
  id: number
  uuid: string
  name: string
  slug: string
  description: string | null
  icon: string | null
  color: string | null
  is_system: boolean
  created_by: string | null
  display_order: number
  description_text: string | null
  hide_titles_from_non_members: boolean
  require_verification: boolean
  created_at: string
  updated_at: string
}

/** A page plus its child subtree (parent_id linkage). */
export interface DocPageNode extends DocPageRow {
  children: DocPageNode[]
}

/** A page is "active" when it is neither archived nor trashed. */
export const isActivePage = (p: DocPageRow): boolean =>
  p.status !== 'deleted' && p.status !== 'archived'

/** Order comparator: display_order first (unset sinks last), then title. */
function byOrderThenTitle(a: DocPageRow, b: DocPageRow): number {
  const oa = a.display_order ?? 999
  const ob = b.display_order ?? 999
  if (oa !== ob) return oa - ob
  return (a.title || '').localeCompare(b.title || '')
}

/**
 * Build a parent_id tree from a flat page list. A page roots when its
 * parent is absent from the set (filtered out, in another collection,
 * or genuinely top-level). Sorted at every level.
 */
export function buildPageTree(rows: DocPageRow[]): DocPageNode[] {
  const nodes = new Map<number, DocPageNode>()
  for (const r of rows) nodes.set(r.id, { ...r, children: [] })
  const roots: DocPageNode[] = []
  for (const node of nodes.values()) {
    const parent = node.parent_id != null ? nodes.get(node.parent_id) : undefined
    if (parent) parent.children.push(node)
    else roots.push(node)
  }
  const sortRec = (list: DocPageNode[]) => {
    list.sort(byOrderThenTitle)
    for (const n of list) if (n.children.length) sortRec(n.children)
  }
  sortRec(roots)
  return roots
}

export const useSyncDocsStore = defineStore('syncDocs', () => {
  const allPages = useAggregate<DocPageRow>('documentation_page')
  const allCollections = useAggregate<DocCollectionRow>('documentation_collection')

  function pageById(id: MaybeRefOrGetter<number | null>): ComputedRef<DocPageRow | null> {
    return useEntity<DocPageRow>('documentation_page', () => toValue(id))
  }

  function collectionById(
    id: MaybeRefOrGetter<number | null>,
  ): ComputedRef<DocCollectionRow | null> {
    return useEntity<DocCollectionRow>('documentation_collection', () => toValue(id))
  }

  function collectionBySlug(
    slug: MaybeRefOrGetter<string | null>,
  ): ComputedRef<DocCollectionRow | null> {
    return computed(() => {
      const s = toValue(slug)
      if (!s) return null
      return allCollections.value.find((c) => c.slug === s) ?? null
    })
  }

  // Collections in nav/index display order.
  const collectionsSorted = computed(() =>
    [...allCollections.value].sort(
      (a, b) => a.display_order - b.display_order || a.name.localeCompare(b.name),
    ),
  )

  /** Flat pages belonging to a collection (any status). */
  function pagesInCollection(
    collectionId: MaybeRefOrGetter<number | null>,
  ): ComputedRef<DocPageRow[]> {
    return computed(() => {
      const cid = toValue(collectionId)
      if (cid == null) return []
      return allPages.value.filter((p) => p.collection_id === cid)
    })
  }

  /** Active (non-archived, non-trashed) page tree for a collection. */
  function treeForCollection(
    collectionId: MaybeRefOrGetter<number | null>,
  ): ComputedRef<DocPageNode[]> {
    return computed(() => {
      const cid = toValue(collectionId)
      if (cid == null) return []
      return buildPageTree(
        allPages.value.filter((p) => p.collection_id === cid && isActivePage(p)),
      )
    })
  }

  /** Pages filtered by status (e.g. 'draft', 'archived', 'deleted'). */
  function pagesByStatus(status: MaybeRefOrGetter<string>): ComputedRef<DocPageRow[]> {
    return computed(() => {
      const s = toValue(status)
      return allPages.value.filter((p) => p.status === s)
    })
  }

  /** Active pages not assigned to any collection. */
  const uncollectedPages = computed(() =>
    allPages.value.filter((p) => p.collection_id == null && isActivePage(p)),
  )

  return {
    allPages,
    allCollections,
    pageById,
    collectionById,
    collectionBySlug,
    collectionsSorted,
    pagesInCollection,
    treeForCollection,
    pagesByStatus,
    uncollectedPages,
  }
})
