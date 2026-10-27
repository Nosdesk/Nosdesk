<script setup lang="ts">
/**
 * The table menus and the floating selection bar of the collaborative
 * editor. The handles live in the table node view (`tableView.ts`); they,
 * and the toolbar's Table button, open menus through `openMenu` and
 * `openGrouped`. Menus go through `ResponsiveMenu`: a popover at desktop
 * widths, a bottom sheet on a phone.
 *
 * The selection bar floats over a cell selection of more than one cell (or
 * a merged cell) on pointer devices, because merge, split and align are
 * edits you want to see land. On touch the toolbar's Table button carries
 * the same actions in its sheet.
 */
import { computed, nextTick, ref, shallowRef, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import type { EditorView } from 'prosemirror-view'
import { undo } from 'y-prosemirror'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import MenuList from '@/components/common/MenuList.vue'
import { ICON_REGISTRY } from '@/components/common/icons'
import { useToastStore } from '@nosdesk/core/stores/toast'
import { useEventListener } from '@/composables/useEventListener'
import {
  columnMenu,
  groupedMenu,
  rowMenu,
  selectionMenu,
  tableMenu,
  type TableMenuContext,
  type TableMenuItem,
  type TableUiState,
} from './tableMenus'
import {
  alignSelection,
  clearCells,
  deleteTable,
  mergeCells,
  selectionAlignment,
  splitCell,
  tableOfSelection,
  tableToMarkdown,
} from './tableCommands'
import type { CellAlign } from './tableCommands'
import type { TableMenuKind } from './tableView'

const props = defineProps<{
  /** The live editor view. */
  view: () => EditorView | null
  /** Where the selection is in a table, null outside one or when read only. */
  ui: TableUiState | null
  /** Touch layout: no selection bar. */
  touch: boolean
}>()

const emit = defineEmits<{ (e: 'open-change', open: boolean): void }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const toast = useToastStore()
const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad|iPod/.test(navigator.userAgent)

const focusEditor = () => {
  // After the menu has closed and handed focus back to its anchor.
  setTimeout(() => props.view()?.focus(), 50)
}

const removeTable = (view: EditorView) => {
  if (!deleteTable(view.state, view.dispatch)) return
  announceDeleted()
}

/** "Table deleted", with an Undo. Also after Backspace on a selected table. */
function announceDeleted() {
  toast.success(t('editor-table-deleted'), undefined, {
    label: t('editor-table-undo'),
    handler: () => {
      const view = props.view()
      if (!view) return
      undo(view.state)
      view.focus()
    },
  })
}

const copyMarkdown = async (view: EditorView) => {
  const rect = tableOfSelection(view.state)
  if (!rect) return
  const { text, markdown } = tableToMarkdown(rect.table)
  try {
    await navigator.clipboard.writeText(text)
  } catch {
    toast.error(t('editor-table-copy-failed'))
    return
  }
  if (markdown) toast.success(t('editor-table-copied-markdown'))
  else toast.success(t('editor-table-copied-html'), t('editor-table-copied-html-detail'))
}


const context = computed<TableMenuContext>(() => ({
  t,
  modKey: isMac ? '⌘' : 'Ctrl',
  deleteTable: removeTable,
  copyMarkdown: (view) => void copyMarkdown(view),
}))

interface OpenMenu {
  title: string
  items: TableMenuItem[]
  anchor: HTMLElement
}

const menu = shallowRef<OpenMenu | null>(null)
// Each menu mounts fresh with its anchor: the popover takes its reference
// element when it mounts, and a handle is a different element every time.
const menuKey = ref(0)
const menuAnchor = computed(() => {
  const anchor = menu.value?.anchor ?? null
  return { type: 'element' as const, element: () => anchor }
})

watch(
  () => menu.value !== null,
  (open) => emit('open-change', open),
)

/** Open the row, column or table menu for the table holding the selection. */
function openMenu(kind: TableMenuKind, index: number, anchor: HTMLElement) {
  const view = props.view()
  if (!view) return
  const items =
    kind === 'row'
      ? rowMenu(view.state, index, context.value)
      : kind === 'column'
        ? columnMenu(view.state, index, context.value)
        : tableMenu(view.state, context.value)
  if (!items.length) return
  const title =
    kind === 'row'
      ? t('editor-table-menu-row', { row: index + 1 })
      : kind === 'column'
        ? t('editor-table-menu-column', { column: index + 1 })
        : t('editor-table-menu-table')
  menuKey.value++
  menu.value = { title, items, anchor }
}

/** The toolbar's Table menu: selection, row, column and table, grouped. */
function openGrouped(anchor: HTMLElement) {
  const view = props.view()
  if (!view) return
  const items = groupedMenu(view.state, context.value)
  if (!items.length) return
  menuKey.value++
  menu.value = { title: t('editor-table-menu-table'), items, anchor }
}

function closeMenu() {
  menu.value = null
}

function onSelect(id: string) {
  const item = menu.value?.items.find((i) => i.id === id)
  const view = props.view()
  menu.value = null
  if (!item?.run || !view) return
  // Synchronously, inside the click: Safari only lets a click write the clipboard.
  item.run(view)
  focusEditor()
}

defineExpose({ openMenu, openGrouped, closeMenu, announceDeleted })

// ---- Selection bar -------------------------------------------------------

const bar = ref<HTMLElement | null>(null)
const barPosition = ref<{ left: number; top: number } | null>(null)
const showBar = computed(() => !props.touch && !!props.ui?.cells && menu.value === null)
const barState = shallowRef<{ multi: boolean; merged: boolean; align: CellAlign }>({
  multi: false,
  merged: false,
  align: 'left',
})

function selectedBox(view: EditorView): DOMRect | null {
  const rect = tableOfSelection(view.state)
  if (!rect) return null
  let box: { left: number; top: number; right: number; bottom: number } | null = null
  const seen = new Set<number>()
  for (let r = rect.top; r < rect.bottom; r++) {
    for (let c = rect.left; c < rect.right; c++) {
      const pos = rect.map.map[r * rect.map.width + c]!
      if (seen.has(pos)) continue
      seen.add(pos)
      const dom = view.nodeDOM(rect.tableStart + pos) as HTMLElement | null
      if (!dom?.getBoundingClientRect) continue
      const b = dom.getBoundingClientRect()
      box = box
        ? {
            left: Math.min(box.left, b.left),
            top: Math.min(box.top, b.top),
            right: Math.max(box.right, b.right),
            bottom: Math.max(box.bottom, b.bottom),
          }
        : { left: b.left, top: b.top, right: b.right, bottom: b.bottom }
    }
  }
  return box ? new DOMRect(box.left, box.top, box.right - box.left, box.bottom - box.top) : null
}

async function placeBar() {
  const view = props.view()
  if (!showBar.value || !view) {
    barPosition.value = null
    return
  }
  const items = selectionMenu(view.state, context.value)
  barState.value = {
    multi: items.some((i) => i.id === 'cells-merge'),
    merged: items.some((i) => i.id === 'cells-split'),
    align: selectionAlignment(view.state),
  }
  const box = selectedBox(view)
  if (!box) {
    barPosition.value = null
    return
  }
  if (!barPosition.value) barPosition.value = { left: -9999, top: -9999 }
  await nextTick()
  const el = bar.value
  if (!el) return
  const width = el.offsetWidth
  const height = el.offsetHeight
  let left = box.left + box.width / 2 - width / 2
  left = Math.max(8, Math.min(window.innerWidth - width - 8, left))
  // Above the selection; below it when that would go under the page header.
  let top = box.top - height - 8
  if (top < 100) top = box.bottom + 8
  barPosition.value = { left, top }
}

watch(() => [props.ui, showBar.value], placeBar)

let frame = 0
const reposition = () => {
  if (!showBar.value || frame) return
  frame = requestAnimationFrame(() => {
    frame = 0
    void placeBar()
  })
}
useEventListener(window, 'resize', reposition, { when: showBar })
useEventListener(window, 'scroll', reposition, { when: showBar, capture: true })

function onBar(action: 'merge' | 'split' | CellAlign | 'clear') {
  const view = props.view()
  if (!view) return
  const { state, dispatch } = view
  if (action === 'merge') mergeCells(state, dispatch)
  else if (action === 'split') splitCell(state, dispatch)
  else if (action === 'clear') clearCells(state, dispatch)
  else alignSelection(action)(state, dispatch)
  view.focus()
}

function onMore(event: MouseEvent) {
  openGrouped(event.currentTarget as HTMLElement)
}

const iconPath = (name: keyof typeof ICON_REGISTRY) => ICON_REGISTRY[name].d
</script>

<template>
  <ResponsiveMenu
    v-if="menu"
    :key="menuKey"
    :open="true"
    :anchor="menuAnchor"
    :title="menu.title"
    placement="bottom-start"
    react-to-scroll="reposition"
    role="menu"
    popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[200px] max-h-[calc(100vh-16px)] overflow-y-auto"
    @close="closeMenu"
  >
    <MenuList :items="menu.items" @select="onSelect" />
  </ResponsiveMenu>

  <Teleport to="body">
    <div
      v-if="showBar && barPosition"
      ref="bar"
      class="table-selection-bar shadow-lg"
      role="toolbar"
      :aria-label="t('editor-table-menu-cells')"
      :style="{ left: `${barPosition.left}px`, top: `${barPosition.top}px` }"
      @mousedown.prevent
    >
      <button v-if="barState.multi" type="button" class="table-selection-bar__button" :title="t('editor-table-merge')" @click="onBar('merge')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path :d="iconPath('merge')" /></svg>
        <span>{{ t('editor-table-bar-merge') }}</span>
      </button>
      <button v-if="barState.merged" type="button" class="table-selection-bar__button" :title="t('editor-table-split')" @click="onBar('split')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path :d="iconPath('split')" /></svg>
        <span>{{ t('editor-table-bar-split') }}</span>
      </button>
      <span class="table-selection-bar__sep" aria-hidden="true"></span>
      <button
        v-for="align in (['left', 'center', 'right'] as const)"
        :key="align"
        type="button"
        class="table-selection-bar__button"
        :class="{ 'is-on': barState.align === align }"
        :aria-pressed="barState.align === align"
        :aria-label="t(`editor-table-align-${align}`)"
        :title="t(`editor-table-align-${align}`)"
        @click="onBar(align)"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path :d="iconPath(align === 'left' ? 'alignLeft' : align === 'center' ? 'alignCenter' : 'alignRight')" />
        </svg>
      </button>
      <span class="table-selection-bar__sep" aria-hidden="true"></span>
      <button type="button" class="table-selection-bar__button" :title="t('editor-table-bar-clear-title')" @click="onBar('clear')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path :d="iconPath('eraser')" /></svg>
        <span>{{ t('editor-table-bar-clear') }}</span>
      </button>
      <button
        type="button"
        class="table-selection-bar__button"
        aria-haspopup="menu"
        :aria-label="t('editor-table-bar-more')"
        :title="t('editor-table-bar-more')"
        @click="onMore"
      >
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path :d="iconPath('more')" transform="rotate(90 12 12)" /></svg>
      </button>
    </div>
  </Teleport>
</template>

<style>
.table-selection-bar {
  position: fixed;
  z-index: calc(var(--z-overlay) - 10);
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 3px;
  background-color: var(--color-surface);
  border: 1px solid var(--color-default);
  border-radius: 0.5rem;
  animation: table-selection-bar-in 150ms cubic-bezier(0.32, 0.72, 0, 1);
}

@keyframes table-selection-bar-in {
  from { opacity: 0; transform: translateY(-6px) scale(0.97); }
  to { opacity: 1; transform: none; }
}

.table-selection-bar__button {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 28px;
  padding: 0 8px;
  border: 0;
  border-radius: 0.375rem;
  background: none;
  font: inherit;
  font-size: 0.75rem;
  font-weight: 500;
  color: var(--color-secondary);
  white-space: nowrap;
  cursor: pointer;
}

.table-selection-bar__button svg {
  width: 14px;
  height: 14px;
  flex-shrink: 0;
}

.table-selection-bar__button:hover {
  background-color: var(--color-surface-hover);
  color: var(--color-primary);
}

.table-selection-bar__button.is-on {
  color: var(--color-accent);
  background-color: var(--color-accent-muted);
}

.table-selection-bar__button:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: 1px;
}

.table-selection-bar__sep {
  width: 1px;
  height: 18px;
  margin: 0 3px;
  background-color: var(--color-default);
}

@media (prefers-reduced-motion: reduce) {
  .table-selection-bar { animation: none; }
}
</style>
