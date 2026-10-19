/**
 * The Pages step's tree as rules, kept out of the components so they can be
 * tested alone: the tree from the server's flat pages, ticks that children
 * follow, leave-out patterns matched as the server matches them, and the
 * options the step sends.
 *
 * The server has one way to leave pages out: `leave_out`, a list of globs
 * over archive paths. A tick is turned into patterns at Check: an unticked
 * page's own file, exactly (a leading `/` anchors it to the archive root),
 * and its folder with everything under it. Those patterns are kept apart
 * from the ones the admin types, which are the only ones shown as chips, and
 * the two are told apart again when a checked job is resumed.
 *
 * Pages are keyed by `path`: reading the tree again (another profile) gives
 * every page a new index, but the same file keeps its path.
 */
import type {
  DocsCollectionMode,
  DocsOptions,
  DocsProfile,
  DocsTreePage,
  DocsTreeView,
} from '@nosdesk/core/services/docsImportService'

/** The server's cap on `leave_out`: patterns, and characters in each. */
export const MAX_PATTERNS = 100
export const MAX_PATTERN_CHARS = 500

export interface TreeNode {
  page: DocsTreePage
  /** 0 for a top-level page. */
  level: number
  children: TreeNode[]
  parent: TreeNode | null
}

export interface Tree {
  roots: TreeNode[]
  /** Every node, parents before children, in display order. */
  all: TreeNode[]
  byPath: Map<string, TreeNode>
  byIndex: Map<number, TreeNode>
}

/** The tree from the server's flat pages, siblings in their order. */
export function buildTree(pages: DocsTreePage[]): Tree {
  const byIndex = new Map<number, TreeNode>()
  for (const page of pages) byIndex.set(page.index, { page, level: 0, children: [], parent: null })
  const roots: TreeNode[] = []
  for (const node of byIndex.values()) {
    const parent = node.page.parent != null ? byIndex.get(node.page.parent) : undefined
    if (parent && parent !== node) {
      node.parent = parent
      parent.children.push(node)
    } else {
      roots.push(node)
    }
  }
  const byOrder = (a: TreeNode, b: TreeNode) => a.page.order - b.page.order || a.page.index - b.page.index
  const all: TreeNode[] = []
  const byPath = new Map<string, TreeNode>()
  const walk = (nodes: TreeNode[], level: number) => {
    nodes.sort(byOrder)
    for (const node of nodes) {
      node.level = level
      all.push(node)
      byPath.set(node.page.path, node)
      walk(node.children, level + 1)
    }
  }
  walk(roots, 0)
  return { roots, all, byPath, byIndex }
}

export function descendants(node: TreeNode): TreeNode[] {
  return node.children.flatMap((c) => [c, ...descendants(c)])
}

export function ancestors(node: TreeNode): TreeNode[] {
  const out: TreeNode[] = []
  for (let at = node.parent; at; at = at.parent) out.push(at)
  return out
}

// ---- globs: the server's `glob_match`, ported ----

type GlobToken =
  | { t: 'byte'; c: string }
  | { t: 'one' }
  | { t: 'star' }
  | { t: 'any' }
  | { t: 'folders' }

function globTokens(pattern: string): GlobToken[] {
  const tokens: GlobToken[] = []
  for (let i = 0; i < pattern.length; ) {
    const c = pattern[i]
    if (c === '*' && pattern[i + 1] === '*') {
      if (pattern[i + 2] === '/') {
        tokens.push({ t: 'folders' })
        i += 3
      } else {
        tokens.push({ t: 'any' })
        i += 2
      }
    } else if (c === '*') {
      tokens.push({ t: 'star' })
      i += 1
    } else if (c === '?') {
      tokens.push({ t: 'one' })
      i += 1
    } else {
      tokens.push({ t: 'byte', c })
      i += 1
    }
  }
  return tokens
}

function globAll(tokens: GlobToken[], s: string): boolean {
  const n = s.length
  let next: boolean[] = new Array(n + 1).fill(false)
  next[n] = true
  for (let k = tokens.length - 1; k >= 0; k--) {
    const token = tokens[k]
    const row: boolean[] = new Array(n + 1).fill(false)
    let slashThenMatch = false
    for (let j = n; j >= 0; j--) {
      switch (token.t) {
        case 'byte':
          row[j] = j < n && s[j] === token.c && next[j + 1]
          break
        case 'one':
          row[j] = j < n && s[j] !== '/' && next[j + 1]
          break
        case 'star':
          row[j] = next[j] || (j < n && s[j] !== '/' && row[j + 1])
          break
        case 'any':
          row[j] = next[j] || (j < n && row[j + 1])
          break
        case 'folders':
          if (j < n && s[j] === '/' && next[j + 1]) slashThenMatch = true
          row[j] = next[j] || (j < n && slashThenMatch)
          break
      }
    }
    next = row
  }
  return next[0]
}

/** Whether `pattern` matches the archive path, as the server decides it:
 *  `**` crosses folders, `*` and `?` don't, and a pattern with no `/`
 *  matches any one name in the path. */
export function globMatch(pattern: string, path: string): boolean {
  const p = pattern.trim()
  if (!p || p.length > 512) return false
  const tokens = globTokens(p.replace(/^\/+/, ''))
  if (!p.includes('/')) return path.split('/').some((segment) => globAll(tokens, segment))
  return globAll(tokens, path)
}

/** The first pattern that leaves this page out, if any: the server leaves
 *  out files, so a page goes with its file, and a section page with no file
 *  goes when a pattern takes everything in its folder. */
export function leftOutBy(node: TreeNode, patterns: string[]): string | null {
  const { file, folder } = node.page
  for (const pattern of patterns) {
    if (file) {
      if (globMatch(pattern, file)) return pattern
    } else if (folder && globMatch(pattern, `${folder}/x.md`) && globMatch(pattern, `${folder}/x/y.md`)) {
      return pattern
    }
  }
  return null
}

// ---- ticks ----

/** Paths of the pages unticked themselves. A page is off when it, or a page
 *  above it, is. */
export type OffSet = ReadonlySet<string>

export function isSelfOff(node: TreeNode, off: OffSet): boolean {
  return off.has(node.page.path)
}

export function isOff(node: TreeNode, off: OffSet): boolean {
  return isSelfOff(node, off) || ancestors(node).some((a) => isSelfOff(a, off))
}

/** On, with some page under it unticked: the tick shows a dash. */
export function isMixed(node: TreeNode, off: OffSet): boolean {
  return !isOff(node, off) && descendants(node).some((d) => isSelfOff(d, off))
}

/** The ticks after clicking this page's: an unticked page comes back; a
 *  mixed one ticks everything under it; a ticked one goes off, with its
 *  children. */
export function toggle(node: TreeNode, off: OffSet): Set<string> {
  const next = new Set(off)
  if (isSelfOff(node, next)) next.delete(node.page.path)
  else if (isMixed(node, next)) for (const d of descendants(node)) next.delete(d.page.path)
  else next.add(node.page.path)
  return next
}

export interface TreeCounts {
  total: number
  /** Pages that will be checked: ticked and not left out. */
  ticked: number
  unticked: number
  leftOut: number
}

export function treeCounts(tree: Tree, off: OffSet, patterns: string[]): TreeCounts {
  let ticked = 0
  let unticked = 0
  let leftOut = 0
  for (const node of tree.all) {
    if (leftOutBy(node, patterns)) leftOut++
    else if (isOff(node, off)) unticked++
    else ticked++
  }
  return { total: tree.all.length, ticked, unticked, leftOut }
}

/** The patterns that leave the unticked pages out: for each unticked page
 *  whose parent is ticked, its file exactly and its folder with everything
 *  in it, plus any page under it whose file sits elsewhere. */
export function tickPatterns(tree: Tree, off: OffSet): string[] {
  const out: string[] = []
  const add = (p: string) => {
    if (!out.includes(p)) out.push(p)
  }
  for (const node of tree.all) {
    if (!isSelfOff(node, off) || ancestors(node).some((a) => isSelfOff(a, off))) continue
    const folder = node.page.folder
    if (node.page.file) add(`/${node.page.file}`)
    if (folder) add(`/${folder}/**`)
    for (const d of descendants(node)) {
      const file = d.page.file
      if (file && !(folder && file.startsWith(`${folder}/`))) add(`/${file}`)
      if (d.page.folder && !(folder && d.page.folder.startsWith(`${folder}/`))) add(`/${d.page.folder}/**`)
    }
  }
  return out
}

/** A stored `leave_out` told apart again: patterns a tick made (an exact
 *  file or folder of a page in the tree) become unticked pages, the rest
 *  are the admin's. */
export function splitPatterns(tree: Tree, stored: string[]): { off: Set<string>; typed: string[] } {
  const byFile = new Map<string, TreeNode>()
  const byFolder = new Map<string, TreeNode>()
  for (const node of tree.all) {
    if (node.page.file) byFile.set(node.page.file, node)
    if (node.page.folder) byFolder.set(node.page.folder, node)
  }
  const off = new Set<string>()
  const typed: string[] = []
  for (const pattern of stored) {
    const file = pattern.startsWith('/') ? byFile.get(pattern.slice(1)) : undefined
    const folder =
      pattern.startsWith('/') && pattern.endsWith('/**') ? byFolder.get(pattern.slice(1, -3)) : undefined
    const node = file ?? folder
    if (node) off.add(node.page.path)
    else typed.push(pattern)
  }
  // Only the top unticked page of a branch needs to stay unticked itself.
  for (const path of [...off]) {
    const node = tree.byPath.get(path)
    if (node && ancestors(node).some((a) => off.has(a.page.path))) off.delete(path)
  }
  return { off, typed }
}

/** Why the patterns can't be sent, if they can't: the server takes at most
 *  100, each up to 500 characters. */
export function patternsProblem(patterns: string[]): 'too_many' | 'too_long' | null {
  if (patterns.length > MAX_PATTERNS) return 'too_many'
  if (patterns.some((p) => p.length > MAX_PATTERN_CHARS)) return 'too_long'
  return null
}

// ---- titles and files ----

const NOTION_ID = /\s+[0-9a-f]{32}$/i
const NUMBER_PREFIX = /^\d+[\s._-]+/

/** The file's name when it says something the title doesn't (the title
 *  came from the page's heading or front matter); null when it is only the
 *  title with an id suffix, a number prefix or an extension. A Notion id is
 *  shortened to its first eight characters. */
export function informativeFile(page: DocsTreePage): string | null {
  if (!page.file) return null
  const base = page.file.split('/').pop() ?? page.file
  const stem = base.replace(/\.(md|markdown|mdx|csv)$/i, '')
  if (/^(index|readme|_index)$/i.test(stem)) return null
  const cleaned = stem
    .replace(NOTION_ID, '')
    .replace(NUMBER_PREFIX, '')
    .replace(/[-_]+/g, ' ')
    .trim()
    .toLowerCase()
  if (cleaned === page.title.trim().toLowerCase()) return null
  return base.replace(/\s([0-9a-f]{8})[0-9a-f]{24}(?=\.|$)/i, ' $1')
}

// ---- destination ----

/** The collection mode the server uses when none is chosen. */
export function defaultMode(profile: DocsProfile): DocsCollectionMode {
  if (profile === 'nosdesk_v2') return 'as_source'
  if (profile === 'outline') return 'per_top_level_folder'
  return 'new'
}

/** Whether a mode lays the tree out differently, so the server must read it
 *  again: New and Existing share one layout. */
export function layoutOf(mode: DocsCollectionMode): 'single' | 'per_folder' | 'as_source' {
  if (mode === 'per_top_level_folder') return 'per_folder'
  if (mode === 'as_source') return 'as_source'
  return 'single'
}

/** What the Pages step holds, as the options it sends. */
export interface PagesChoices {
  profile: DocsProfile | null
  mode: DocsCollectionMode
  collectionName: string
  collectionId: number | null
  underPageId: number | null
  status: NonNullable<DocsOptions['status']>
  webImages: NonNullable<DocsOptions['web_images']>
  duplicates: NonNullable<DocsOptions['duplicates']>
  /** The admin's own patterns. */
  typed: string[]
}

/** The options for a check (`leave_out` holds the typed patterns and the
 *  ticks'), or for reading the tree again (`leave_out` empty, so unticked
 *  pages stay in the tree, greyed). */
export function optionsPayload(
  choices: PagesChoices,
  tickPats: string[],
  purpose: 'check' | 'reread',
): DocsOptions {
  const options: DocsOptions = {
    collections: choices.mode,
    status: choices.status,
    web_images: choices.webImages,
    duplicates: choices.duplicates,
    leave_out: purpose === 'check' ? [...choices.typed, ...tickPats.filter((p) => !choices.typed.includes(p))] : [],
  }
  if (choices.profile) options.profile = choices.profile
  if (choices.mode === 'existing') {
    options.collection_id = choices.collectionId
    if (choices.underPageId != null) options.under_page_id = choices.underPageId
  } else if (choices.mode === 'new' && choices.collectionName.trim()) {
    options.collection_name = choices.collectionName.trim()
  }
  return options
}

/** The tree's top-level groups under the chosen destination: one head per
 *  collection the pages go in, with its pages. */
export interface CollectionGroup {
  key: string
  name: string
  kind: 'new' | 'existing' | 'none'
  nodes: TreeNode[]
}

export function collectionGroups(
  tree: Tree,
  view: DocsTreeView,
  choices: Pick<PagesChoices, 'mode' | 'collectionName'>,
  existing: { name: string; under?: string | null } | null,
): CollectionGroup[] {
  if (choices.mode === 'existing') {
    const name = existing ? (existing.under ? `${existing.name} / ${existing.under}` : existing.name) : ''
    return [{ key: 'existing', name, kind: 'existing', nodes: tree.roots }]
  }
  if (choices.mode === 'new') {
    const name = choices.collectionName.trim() || view.collections[0] || ''
    return [{ key: 'new', name, kind: 'new', nodes: tree.roots }]
  }
  const groups: CollectionGroup[] = view.collections.map((name, i) => ({
    key: `c${i}`,
    name,
    kind: 'new' as const,
    nodes: [],
  }))
  const loose: TreeNode[] = []
  for (const node of tree.roots) {
    const c = node.page.collection
    if (c != null && groups[c]) groups[c].nodes.push(node)
    else loose.push(node)
  }
  if (loose.length > 0) groups.push({ key: 'none', name: '', kind: 'none', nodes: loose })
  return groups.filter((g) => g.nodes.length > 0)
}

/** An archive path as people read it: Notion's 32-character ids cut to
 *  their first eight. */
export function shortPath(path: string): string {
  return path.replace(/\s([0-9a-f]{8})[0-9a-f]{24}(?=\/|\.|$)/gi, ' $1')
}
