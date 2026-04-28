import type { CollectionPage, CollectionPageTreeNode } from '@/services/collectionService'
import type { Page } from '@/services/documentationService'

/**
 * Generic recursive tree search by ID.
 * Works with any node type that has `id` and optional `children`.
 */
export function findInTree<T extends { id: string | number; children?: T[] }>(
  tree: T[],
  id: string | number,
): T | null {
  for (const node of tree) {
    if (String(node.id) === String(id)) return node
    if (node.children && node.children.length > 0) {
      const found = findInTree(node.children, id)
      if (found) return found
    }
  }
  return null
}

/**
 * Sort comparator for pages: by display_order first, then by title.
 */
export const sortByOrder = (
  a: { display_order?: number | null; title?: string },
  b: { display_order?: number | null; title?: string }
) => {
  const orderA = a.display_order !== undefined && a.display_order !== null ? Number(a.display_order) : 999
  const orderB = b.display_order !== undefined && b.display_order !== null ? Number(b.display_order) : 999
  if (orderA !== orderB) return orderA - orderB
  return (a.title || '').localeCompare(b.title || '')
}

/**
 * Build a tree of Page objects from a flat list of CollectionPage items.
 * Filters out deleted and archived pages. A page is a root if its parent_id
 * is null or its parent is not in the list.
 */
export const buildTreeFromFlat = (flatPages: CollectionPage[]): Page[] => {
  const activeFlatPages = flatPages.filter(p => p.status !== 'deleted' && p.status !== 'archived')

  const pageMap: Record<string, Page> = {}
  const pageIdSet = new Set(activeFlatPages.map(p => String(p.id)))

  for (const p of activeFlatPages) {
    pageMap[String(p.id)] = {
      id: p.id,
      uuid: p.uuid,
      slug: p.slug || '',
      title: p.title,
      description: null,
      content: '',
      parent_id: p.parent_id,
      author: '',
      status: p.status,
      icon: p.icon,
      children: [],
      display_order: p.display_order ?? undefined,
    }
  }

  const roots: Page[] = []
  for (const p of activeFlatPages) {
    const parentId = p.parent_id ? String(p.parent_id) : null
    if (parentId === null || !pageIdSet.has(parentId)) {
      roots.push(pageMap[String(p.id)])
    } else {
      pageMap[parentId].children.push(pageMap[String(p.id)])
    }
  }

  const sortRecursive = (pages: Page[]) => {
    pages.sort(sortByOrder)
    for (const p of pages) {
      if (p.children.length > 0) sortRecursive(p.children)
    }
  }
  sortRecursive(roots)

  return roots
}

/**
 * Build a tree of CollectionPageTreeNode from flat CollectionPage items.
 * Similar to buildTreeFromFlat but preserves the CollectionPageTreeNode type
 * (without converting to Page).
 */
export const buildCollectionTree = (flatPages: CollectionPage[]): CollectionPageTreeNode[] => {
  const activeFlatPages = flatPages.filter(p => p.status !== 'deleted' && p.status !== 'archived')

  const nodeMap: Record<string, CollectionPageTreeNode> = {}
  const pageIdSet = new Set(activeFlatPages.map(p => String(p.id)))

  for (const p of activeFlatPages) {
    nodeMap[String(p.id)] = { ...p, children: [] }
  }

  const roots: CollectionPageTreeNode[] = []
  for (const p of activeFlatPages) {
    const parentId = p.parent_id ? String(p.parent_id) : null
    if (parentId === null || !pageIdSet.has(parentId)) {
      roots.push(nodeMap[String(p.id)])
    } else {
      nodeMap[parentId].children.push(nodeMap[String(p.id)])
    }
  }

  const sortRecursive = (nodes: CollectionPageTreeNode[]) => {
    nodes.sort(sortByOrder)
    for (const n of nodes) {
      if (n.children.length > 0) sortRecursive(n.children)
    }
  }
  sortRecursive(roots)

  return roots
}

/**
 * Walk the parent_id chain upward from a given page, returning ancestors
 * from root to immediate parent (does NOT include the page itself).
 */
export const getAncestorChain = (
  pageId: number | string,
  flatPages: CollectionPage[]
): CollectionPage[] => {
  const pageMap = new Map<string, CollectionPage>()
  for (const p of flatPages) {
    pageMap.set(String(p.id), p)
  }

  const ancestors: CollectionPage[] = []
  const currentId = String(pageId)
  const current = pageMap.get(currentId)
  if (!current) return ancestors

  let parentId = current.parent_id ? String(current.parent_id) : null

  // Walk up, collecting ancestors (guard against cycles)
  const visited = new Set<string>()
  while (parentId && pageMap.has(parentId) && !visited.has(parentId)) {
    visited.add(parentId)
    ancestors.unshift(pageMap.get(parentId)!)
    const parent = pageMap.get(parentId)!
    parentId = parent.parent_id ? String(parent.parent_id) : null
  }

  return ancestors
}
