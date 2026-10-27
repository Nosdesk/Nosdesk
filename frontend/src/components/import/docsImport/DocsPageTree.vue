<script setup lang="ts">
/**
 * The pages of a documentation import as a tree, the way Nosdesk will show
 * them. On Pages each page has a tick (children follow their parent, a
 * parent with some children off shows a dash) and pages a leave-out
 * pattern takes are greyed with the pattern; each page shows what it stands
 * for (no content, a Notion database) and its counts of pages and images.
 * On an import's detail page each page carries its outcome instead, and the
 * title of a page the import wrote opens it.
 *
 * A WAI-ARIA tree: one tab stop, arrow keys move, Right and Left expand and
 * collapse (or move to the child or parent), Home and End jump, Space ticks,
 * Enter expands, `*` expands everything. Rows are rendered flat with
 * `aria-level`, so the tree needs no recursive component.
 */
import { computed, nextTick, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { RouterLink } from 'vue-router'

import Icon from '@/components/common/Icon.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import type { StatusPillTone } from '@/components/common/statusPillTone'

import {
  informativeFile,
  isMixed,
  isOff,
  leftOutBy,
  pageKind,
  type CollectionGroup,
  type OffSet,
  type Tree,
  type TreeNode,
} from './docsTreeModel'
import type { RowOutcome } from './docsReviewModel'

/** A page's outcome on the detail page: the check's, or once Remove
 *  imported pages ran, whether it went to Trash or was kept. */
export type TreeOutcome = RowOutcome | 'left' | 'trash' | 'kept'

const props = withDefaults(
  defineProps<{
    tree: Tree
    groups: CollectionGroup[]
    mode?: 'pages' | 'detail'
    /** Paths of pages unticked themselves (Pages). */
    off?: OffSet
    /** Leave-out patterns the admin typed (Pages). */
    patterns?: string[]
    /** Each page's outcome by path (detail). */
    outcomes?: ReadonlyMap<string, TreeOutcome>
    /** Each page's note count by path (detail). */
    noteCounts?: ReadonlyMap<string, number>
    /** The page each written page became, by path (detail). */
    links?: ReadonlyMap<string, number>
    /** Paths of the expanded pages. */
    expanded: ReadonlySet<string>
    label: string
  }>(),
  {
    mode: 'pages',
    off: () => new Set<string>(),
    patterns: () => [],
    outcomes: () => new Map(),
    noteCounts: () => new Map(),
    links: () => new Map(),
  },
)

const emit = defineEmits<{
  toggle: [node: TreeNode]
  'update:expanded': [expanded: Set<string>]
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

interface Row {
  kind: 'head' | 'item'
  key: string
  group?: CollectionGroup
  node?: TreeNode
  level: number
  setSize: number
  posInSet: number
}

/** The rows on screen: each group's head, then its pages down to the
 *  expanded depth. */
const rows = computed<Row[]>(() => {
  const out: Row[] = []
  const walk = (nodes: TreeNode[], level: number) => {
    nodes.forEach((node, i) => {
      out.push({ kind: 'item', key: node.page.path, node, level, setSize: nodes.length, posInSet: i + 1 })
      if (node.children.length > 0 && props.expanded.has(node.page.path)) walk(node.children, level + 1)
    })
  }
  for (const group of props.groups) {
    if (props.groups.length > 1 || group.name) {
      out.push({ kind: 'head', key: `head:${group.key}`, group, level: 0, setSize: 0, posInSet: 0 })
    }
    walk(group.nodes, 0)
  }
  return out
})

const items = computed(() => rows.value.filter((r) => r.kind === 'item'))

const focusPath = ref<string | null>(null)
const effectiveFocus = computed(() => {
  const paths = items.value.map((r) => r.key)
  return focusPath.value && paths.includes(focusPath.value) ? focusPath.value : (paths[0] ?? null)
})

const listRef = ref<HTMLElement | null>(null)

async function focusRow(path: string | undefined) {
  if (!path) return
  focusPath.value = path
  await nextTick()
  const el = listRef.value?.querySelector<HTMLElement>(`[data-path="${CSS.escape(path)}"]`)
  el?.focus()
}

function setExpanded(path: string, open: boolean) {
  const next = new Set(props.expanded)
  if (open) next.add(path)
  else next.delete(path)
  emit('update:expanded', next)
}

function leftOut(node: TreeNode): string | null {
  return props.mode === 'pages' ? leftOutBy(node, props.patterns) : null
}

function tickDisabled(node: TreeNode): boolean {
  return !!leftOut(node) || (!!node.parent && isOff(node.parent, props.off))
}

function tickState(node: TreeNode): 'on' | 'off' | 'mixed' {
  if (leftOut(node) || isOff(node, props.off)) return 'off'
  return isMixed(node, props.off) ? 'mixed' : 'on'
}

function dimmed(node: TreeNode): boolean {
  if (props.mode === 'pages') return !!leftOut(node) || isOff(node, props.off)
  const o = props.outcomes.get(node.page.path)
  return o === 'left' || o === 'skipped'
}

function onTick(node: TreeNode) {
  if (tickDisabled(node)) return
  focusPath.value = node.page.path
  emit('toggle', node)
}

function onKeydown(event: KeyboardEvent, row: Row) {
  const node = row.node!
  const list = items.value
  const ix = list.findIndex((r) => r.key === row.key)
  const hasKids = node.children.length > 0
  const open = props.expanded.has(node.page.path)
  switch (event.key) {
    case 'ArrowDown':
      event.preventDefault()
      void focusRow(list[ix + 1]?.key)
      break
    case 'ArrowUp':
      event.preventDefault()
      void focusRow(list[ix - 1]?.key)
      break
    case 'Home':
      event.preventDefault()
      void focusRow(list[0]?.key)
      break
    case 'End':
      event.preventDefault()
      void focusRow(list[list.length - 1]?.key)
      break
    case 'ArrowRight':
      event.preventDefault()
      if (hasKids && !open) setExpanded(node.page.path, true)
      else if (hasKids) void focusRow(list[ix + 1]?.key)
      break
    case 'ArrowLeft':
      event.preventDefault()
      if (hasKids && open) setExpanded(node.page.path, false)
      else if (node.parent) void focusRow(node.parent.page.path)
      break
    case ' ':
      event.preventDefault()
      if (props.mode === 'pages') onTick(node)
      break
    case 'Enter':
      event.preventDefault()
      if (hasKids) setExpanded(node.page.path, !open)
      break
    case '*': {
      event.preventDefault()
      const next = new Set(props.expanded)
      for (const sibling of node.parent ? node.parent.children : props.tree.roots) {
        if (sibling.children.length > 0) next.add(sibling.page.path)
      }
      emit('update:expanded', next)
      break
    }
  }
}

function onRowClick(row: Row, event: MouseEvent) {
  if ((event.target as HTMLElement).closest('input, label, button')) return
  void focusRow(row.key)
}

const OUTCOME: Record<TreeOutcome, { key: string; tone: StatusPillTone }> = {
  new: { key: 'docs-import-outcome-new', tone: 'positive' },
  copy: { key: 'docs-import-outcome-copy', tone: 'info' },
  skipped: { key: 'docs-import-outcome-skipped', tone: 'neutral' },
  error: { key: 'docs-import-outcome-error', tone: 'critical' },
  left: { key: 'docs-import-outcome-left', tone: 'neutral' },
  trash: { key: 'docs-import-outcome-trash', tone: 'neutral' },
  kept: { key: 'docs-import-outcome-kept', tone: 'caution' },
}

function icon(node: TreeNode) {
  const kind = pageKind(node.page)
  if (kind === 'section') return 'folder'
  if (kind === 'database') return 'table'
  return node.children.length > 0 ? 'book' : 'document'
}

/** The written page a title opens, if any. */
function linkOf(node: TreeNode): number | null {
  return props.mode === 'detail' ? (props.links.get(node.page.path) ?? null) : null
}

function tickLabel(node: TreeNode): string {
  return node.children.length > 0
    ? t('docs-import-tree-tick-with-children', { title: node.page.title, count: node.children.length })
    : t('docs-import-tree-tick', { title: node.page.title })
}

function headSub(group: CollectionGroup): string {
  if (group.kind === 'existing') return t('docs-import-tree-head-existing')
  if (group.kind === 'none') return t('docs-import-tree-head-none')
  return t('docs-import-tree-head-new')
}

// A page that moved out of view takes the tab stop with it to the first row.
watch(items, (list) => {
  if (focusPath.value && !list.some((r) => r.key === focusPath.value)) focusPath.value = null
})
</script>

<template>
  <ul ref="listRef" role="tree" :aria-label="label" class="py-1" data-test="docs-tree">
    <template v-for="row in rows" :key="row.key">
      <li
        v-if="row.kind === 'head'"
        role="presentation"
        class="flex items-center gap-2 px-3 pt-2.5 pb-1.5 text-xs font-semibold uppercase tracking-wide text-tertiary [&:not(:first-child)]:border-t [&:not(:first-child)]:border-default [&:not(:first-child)]:mt-1"
        data-test="docs-tree-head"
      >
        <Icon name="book" size="xs" />
        <span class="truncate">{{ row.group!.name || $t('docs-import-tree-no-collection') }}</span>
        <span class="normal-case tracking-normal font-normal">· {{ headSub(row.group!) }}</span>
      </li>
      <li
        v-else
        role="treeitem"
        :aria-level="row.level + 1"
        :aria-setsize="row.setSize"
        :aria-posinset="row.posInSet"
        :aria-expanded="row.node!.children.length > 0 ? expanded.has(row.key) : undefined"
        :aria-checked="mode === 'pages' ? (tickState(row.node!) === 'mixed' ? 'mixed' : tickState(row.node!) === 'on') : undefined"
        :tabindex="effectiveFocus === row.key ? 0 : -1"
        :data-path="row.key"
        class="group outline-none"
        data-test="docs-tree-item"
        @keydown="onKeydown($event, row)"
        @click="onRowClick(row, $event)"
      >
        <div
          class="flex items-center gap-1.5 sm:gap-2 min-h-9 py-0.5 pr-2 sm:pr-3 rounded-md hover:bg-surface-alt group-focus-visible:ring-2 group-focus-visible:ring-inset group-focus-visible:ring-accent"
          :style="{ paddingLeft: `calc(0.5rem + ${row.level} * var(--docs-tree-indent, 14px))` }"
        >
          <button
            v-if="row.node!.children.length > 0"
            type="button"
            tabindex="-1"
            class="w-[22px] h-[22px] rounded flex items-center justify-center text-tertiary hover:bg-surface-hover hover:text-primary shrink-0"
            :aria-label="expanded.has(row.key) ? $t('docs-import-tree-collapse', { title: row.node!.page.title }) : $t('docs-import-tree-expand', { title: row.node!.page.title })"
            @click="setExpanded(row.key, !expanded.has(row.key)); focusPath = row.key"
          >
            <Icon
              name="chevronRight"
              size="xs"
              class="transition-transform"
              :class="expanded.has(row.key) ? 'rotate-90' : ''"
            />
          </button>
          <span v-else class="w-[22px] shrink-0" />

          <span
            v-if="mode === 'pages'"
            class="docs-chk relative w-[18px] h-[18px] shrink-0 rounded border-2 flex items-center justify-center transition-colors"
            :class="[
              tickState(row.node!) === 'off' ? 'border-strong bg-surface' : 'border-accent bg-accent text-on-accent',
              tickDisabled(row.node!) ? 'opacity-40' : 'hover:border-accent',
            ]"
          >
            <input
              :id="`docs-tick-${row.node!.page.index}`"
              type="checkbox"
              tabindex="-1"
              class="absolute inset-0 w-full h-full m-0 opacity-0"
              :class="tickDisabled(row.node!) ? 'cursor-not-allowed' : 'cursor-pointer'"
              :checked="tickState(row.node!) !== 'off'"
              :indeterminate="tickState(row.node!) === 'mixed'"
              :disabled="tickDisabled(row.node!)"
              :aria-label="tickLabel(row.node!)"
              data-test="docs-tree-tick"
              @change="onTick(row.node!)"
            />
            <Icon v-if="tickState(row.node!) === 'on'" name="check" size="xs" class="pointer-events-none" />
            <span v-else-if="tickState(row.node!) === 'mixed'" class="w-2 h-0.5 rounded bg-current pointer-events-none" />
          </span>

          <Icon :name="icon(row.node!)" size="sm" class="shrink-0" :class="dimmed(row.node!) ? 'text-tertiary opacity-60' : 'text-tertiary'" />

          <span class="flex-1 min-w-0 flex items-baseline gap-2 flex-wrap">
            <label
              v-if="mode === 'pages'"
              :for="`docs-tick-${row.node!.page.index}`"
              class="text-sm min-w-0 break-words cursor-pointer"
              :class="[
                dimmed(row.node!) ? 'text-tertiary opacity-60' : 'text-primary',
                !leftOut(row.node!) && isOff(row.node!, off) ? 'line-through decoration-strong' : '',
              ]"
              data-test="docs-tree-title"
            >{{ row.node!.page.title }}</label>
            <RouterLink
              v-else-if="linkOf(row.node!) != null"
              :to="{ name: 'documentation-page', params: { path: String(linkOf(row.node!)) } }"
              tabindex="-1"
              class="text-sm min-w-0 break-words text-accent hover:underline"
              data-test="docs-tree-link"
            >{{ row.node!.page.title }}</RouterLink>
            <span
              v-else
              class="text-sm min-w-0 break-words"
              :class="dimmed(row.node!) ? 'text-tertiary' : 'text-primary'"
              data-test="docs-tree-title"
            >{{ row.node!.page.title }}</span>
            <span
              v-if="informativeFile(row.node!.page)"
              class="hidden sm:inline text-xs font-mono text-tertiary truncate max-w-full"
              :title="row.node!.page.path"
            >{{ informativeFile(row.node!.page) }}</span>
          </span>

          <span class="flex items-center gap-1.5 shrink-0">
            <template v-if="mode === 'pages'">
              <template v-if="leftOut(row.node!)">
                <StatusPill :label="$t('docs-import-tree-left-out')" tone="neutral" :dot="false" data-test="docs-tree-left-out" />
                <span class="hidden sm:inline text-2xs font-mono text-tertiary">{{ leftOut(row.node!) }}</span>
              </template>
              <StatusPill
                v-else-if="pageKind(row.node!.page) === 'section'"
                :label="$t('docs-import-tree-no-content')"
                tone="neutral"
                :dot="false"
                data-test="docs-tree-no-content"
              />
              <StatusPill
                v-if="pageKind(row.node!.page) === 'database'"
                :label="$t('docs-import-tree-database')"
                tone="info"
                :dot="false"
                data-test="docs-tree-database"
              />
              <span
                v-if="row.node!.children.length > 0 && !leftOut(row.node!)"
                class="hidden sm:inline text-2xs text-tertiary whitespace-nowrap"
              >{{ $t('docs-import-tree-page-count', { count: row.node!.children.length }) }}</span>
              <span
                v-if="(row.node!.page.images ?? 0) > 0 && !leftOut(row.node!)"
                class="hidden sm:inline text-2xs text-tertiary whitespace-nowrap"
                data-test="docs-tree-images"
              >{{ $t('docs-import-tree-image-count', { count: row.node!.page.images ?? 0 }) }}</span>
            </template>
            <template v-else>
              <StatusPill
                v-if="outcomes.get(row.key)"
                :label="$t(OUTCOME[outcomes.get(row.key)!].key)"
                :tone="OUTCOME[outcomes.get(row.key)!].tone"
                data-test="docs-tree-outcome"
              />
              <span
                v-if="(noteCounts.get(row.key) ?? 0) > 0 && outcomes.get(row.key) !== 'skipped'"
                class="hidden sm:inline-flex items-center gap-1 text-2xs text-secondary bg-surface-alt border border-default rounded-full px-1.5 py-0.5 whitespace-nowrap"
              >
                <Icon name="info" size="xs" />
                {{ $t('docs-import-tree-note-count', { count: noteCounts.get(row.key) ?? 0 }) }}
              </span>
            </template>
          </span>
        </div>
      </li>
    </template>
  </ul>
</template>

<style scoped>
@media (min-width: 640px) {
  [role='tree'] {
    --docs-tree-indent: 20px;
  }
}
.docs-chk:has(input:focus-visible) {
  outline: 2px solid var(--color-accent);
  outline-offset: 2px;
}
</style>
