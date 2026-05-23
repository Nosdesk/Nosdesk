<script setup lang="ts" generic="T extends object">
import { computed } from 'vue'
import Checkbox from './Checkbox.vue'

export interface Column {
  field: string
  label: string
  width?: string
  sortable?: boolean
  sortKey?: string // Optional different field name for API sorting (defaults to field)
  responsive?: 'always' | 'md' | 'lg' // Show only on certain breakpoints
}

export interface DataTableBucket<T> {
  key: string
  label: string
  items: readonly T[]
}

/** Header drag-reorder bundle. The consumer wires this up via
 *  `useDataTableColumns`; the table marks each non-pinned
 *  header `draggable=true` and forwards the gesture events.
 *  Optional — when omitted, headers are not draggable. */
export interface DataTableColumnReorder {
  sourceId: { value: string | null }
  targetId: { value: string | null }
  isReorderable: (field: string) => boolean
  onDragStart: (field: string, event: DragEvent) => void
  onDragOver: (field: string, event: DragEvent) => void
  onDragLeave: (field: string) => void
  onDrop: (field: string, event: DragEvent) => void
  onDragEnd: () => void
}

/** Header resize bundle. When provided, each header renders a
 *  right-edge drag handle that initiates a pointer-driven
 *  width resize through `useDataTableColumns`. The composable
 *  measures the column's start width by reading the header
 *  element's offsetWidth on pointerdown; the table passes that
 *  through here. Optional — when omitted, headers are not
 *  resizable. */
export interface DataTableColumnResize {
  resizingId: { value: string | null }
  onResizeStart: (
    field: string,
    event: PointerEvent,
    startWidthPx: number,
  ) => void
}

const props = withDefaults(defineProps<{
  columns: readonly Column[]
  data: readonly T[]
  selectedItems: readonly string[]
  itemIdField?: string
  sortField?: string
  sortDirection?: 'asc' | 'desc'
  loading?: boolean
  gridClass?: string
  /** When non-empty, the table renders bucket header rows
   *  interleaved with item rows under each bucket. Use this for
   *  group-by views. When omitted or empty, the existing flat
   *  rendering of `data` applies. */
  buckets?: readonly DataTableBucket<T>[]
  /** Per-bucket fold state. Called for each bucket; collapsed
   *  buckets render the header alone, skipping their items. */
  isCollapsed?: (bucketKey: string) => boolean
  /** Drag-reorder wiring from useDataTableColumns. When set,
   *  headers become draggable and the gesture commits a new
   *  column order through the composable. */
  columnReorder?: DataTableColumnReorder
  /** Column resize wiring from useDataTableColumns. When set,
   *  each header renders a right-edge drag handle for resize. */
  columnResize?: DataTableColumnResize
}>(), {
  itemIdField: 'id',
  loading: false,
  gridClass: '',
})

const emit = defineEmits<{
  'update:sort': [field: string, direction: 'asc' | 'desc']
  'toggle-selection': [event: Event, itemId: string]
  'toggle-all': [event: Event]
  'row-click': [item: T]
  'row-mouseenter': [item: T]
  'toggle-bucket': [bucketKey: string]
}>()

/** True when the consumer asked for grouped rendering. Buckets
 *  with no items are filtered out below so empty groups don't
 *  produce a lone header row. */
const isGrouped = computed(() => (props.buckets?.length ?? 0) > 0)

const visibleBuckets = computed(() =>
  (props.buckets ?? []).filter((b) => b.items.length > 0),
)

/// Read a field from a row by string name. Tables are dynamic by
/// their nature — column.field is a string from the column config —
/// so a runtime index access cast is unavoidable. Centralising the
/// cast here keeps callers clean.
const fieldValue = (item: T, field: string): unknown =>
  (item as Record<string, unknown>)[field]

// Compute if all items are selected
const allSelected = computed(() => {
  if (!props.data.length) return false
  return props.data.every(item =>
    props.selectedItems.includes(String(fieldValue(item, props.itemIdField)))
  )
})

// Handle sort toggle
const toggleSort = (column: Column) => {
  if (!column.sortable) return

  const sortKey = column.sortKey || column.field

  if (props.sortField === sortKey) {
    const newDirection = props.sortDirection === 'asc' ? 'desc' : 'asc'
    emit('update:sort', sortKey, newDirection)
  } else {
    emit('update:sort', sortKey, 'asc')
  }
}

// Helper to check if column is currently sorted
const isColumnSorted = (column: Column) => {
  const sortKey = column.sortKey || column.field
  return props.sortField === sortKey
}

// Get visible columns based on responsive breakpoint
const getVisibleColumns = (breakpoint: 'base' | 'md' | 'lg') => {
  return props.columns.filter(col => {
    // 'always' columns are visible at all breakpoints
    if (!col.responsive || col.responsive === 'always') return true
    // 'md' columns visible at md and lg breakpoints
    if (col.responsive === 'md') return breakpoint === 'md' || breakpoint === 'lg'
    // 'lg' columns only visible at lg breakpoint
    if (col.responsive === 'lg') return breakpoint === 'lg'
    return false
  })
}

// Generate grid-template-columns value for inline styles
const getGridTemplate = (columns: Column[]) => {
  const widths = columns.map(col => col.width || '1fr')
  return `auto ${widths.join(' ')}` // auto for checkbox column
}

// Responsive grid templates
const gridTemplates = computed(() => ({
  base: getGridTemplate(getVisibleColumns('base')),
  md: getGridTemplate(getVisibleColumns('md')),
  lg: getGridTemplate(getVisibleColumns('lg'))
}))

// Helper to determine if column should be visible at current breakpoint
const getColumnVisibility = (column: Column) => {
  if (!column.responsive || column.responsive === 'always') return ''
  if (column.responsive === 'md') return 'hidden md:flex'
  if (column.responsive === 'lg') return 'hidden lg:flex'
  return ''
}
</script>

<template>
  <div
    class="flex flex-col h-full data-table"
    :style="{
      '--grid-cols-base': gridTemplates.base,
      '--grid-cols-md': gridTemplates.md,
      '--grid-cols-lg': gridTemplates.lg
    }"
  >
    <!-- Grid Container -->
    <div class="data-table-grid">
      
      <!-- Sticky Header Row -->
      <div class="contents sticky top-0 z-10">
        <!-- Checkbox Header. Padding + border match the data-
             column headers below so the strip reads as one
             compact band of column chrome. -->
        <div class="px-4 py-2 flex items-center bg-surface border-b border-subtle sticky top-0 z-10">
          <Checkbox
            :model-value="allSelected && data.length > 0"
            @change="(e) => emit('toggle-all', e)"
          />
        </div>

        <!-- Column Headers. Compact uppercase treatment mirroring
             the tickets table: text-[10px] uppercase + tertiary
             text + tracking-wider, with a subtle bottom rule so
             the header row reads as scaffolding rather than
             competing with the data cells. When `columnReorder`
             is provided, non-pinned headers become draggable so
             the user can grab and drop to reorder; the current
             drag-target highlights with a left-edge accent. When
             `columnResize` is provided, each header renders a
             right-edge drag handle for live width resize. -->
        <div
          v-for="column in columns"
          :key="column.field"
          :draggable="columnReorder ? columnReorder.isReorderable(column.field) : false"
          :class="[
            'px-2 py-2 flex items-center text-[10px] font-semibold uppercase tracking-wider text-tertiary bg-surface border-b border-subtle sticky top-0 z-10 relative',
            getColumnVisibility(column),
            column.sortable ? 'cursor-pointer hover:bg-surface-hover hover:text-primary' : '',
            columnReorder?.sourceId.value === column.field ? 'opacity-50' : '',
            columnReorder?.targetId.value === column.field ? 'before:absolute before:left-0 before:top-1 before:bottom-1 before:w-0.5 before:bg-accent before:rounded-r' : '',
          ]"
          @click="toggleSort(column)"
          @dragstart="(e) => columnReorder?.onDragStart(column.field, e)"
          @dragover="(e) => columnReorder?.onDragOver(column.field, e)"
          @dragleave="() => columnReorder?.onDragLeave(column.field)"
          @drop="(e) => columnReorder?.onDrop(column.field, e)"
          @dragend="() => columnReorder?.onDragEnd()"
        >
          <div class="flex items-center gap-1">
            {{ column.label }}
            <span v-if="column.sortable && isColumnSorted(column)" class="text-primary">
              {{ sortDirection === 'asc' ? '↑' : '↓' }}
            </span>
          </div>
          <!-- Resize handle. 4px hit area on the right edge, with
               a thin accent line that brightens on hover and
               while the gesture is active. Reads the parent
               header's offsetWidth on pointerdown to give the
               composable an accurate startValue (the composable
               can't measure DOM itself). -->
          <div
            v-if="columnResize"
            class="absolute top-1 bottom-1 right-0 w-1 cursor-col-resize group/handle"
            :class="columnResize.resizingId.value === column.field
              ? 'bg-accent/50'
              : 'hover:bg-accent/30'"
            :title="$t('views-column-resize-handle-tooltip')"
            @click.stop
            @pointerdown="(e: PointerEvent) => {
              const headerEl = (e.currentTarget as HTMLElement).parentElement
              const startPx = headerEl?.offsetWidth ?? 0
              columnResize!.onResizeStart(column.field, e, startPx)
            }"
          />
        </div>
      </div>

      <!-- Flat rendering: existing behaviour when no buckets. -->
      <template v-if="!isGrouped">
        <template v-for="(item, index) in data" :key="String(fieldValue(item, itemIdField))">
          <div
            class="contents group cursor-pointer"
            @click="emit('row-click', item)"
            @mouseenter="emit('row-mouseenter', item)"
          >
            <!-- Checkbox Cell -->
            <div
              class="px-4 py-3 flex items-center bg-app group-hover:bg-surface-hover"
              :class="[
                loading ? 'opacity-60 pointer-events-none' : 'transition-colors',
                index > 0 ? 'border-t border-default' : ''
              ]"
              @click.stop
            >
              <Checkbox
                :model-value="selectedItems.includes(String(fieldValue(item, itemIdField)))"
                @change="(e) => emit('toggle-selection', e, String(fieldValue(item, itemIdField)))"
              />
            </div>

            <!-- Data Cells -->
            <div
              v-for="column in columns"
              :key="column.field"
              :class="[
                'px-2 py-3 flex items-center bg-app group-hover:bg-surface-hover text-sm min-w-0',
                getColumnVisibility(column),
                loading ? 'opacity-60 pointer-events-none' : 'transition-colors',
                index > 0 ? 'border-t border-default' : ''
              ]"
            >
              <slot
                :name="`cell-${column.field}`"
                :item="item"
                :value="fieldValue(item, column.field)"
                :index="index"
                :column="column"
              >
                <span class="truncate text-primary">
                  {{ fieldValue(item, column.field) }}
                </span>
              </slot>
            </div>
          </div>
        </template>
      </template>

      <!-- Grouped rendering: bucket header row + items per bucket.
           The header spans all columns via `grid-column: 1 / -1`
           so it reads as a single full-width row inside the same
           grid; this keeps the column-header alignment that
           rendering N small tables would lose. -->
      <template v-else>
        <template v-for="(bucket, bIdx) in visibleBuckets" :key="bucket.key">
          <button
            type="button"
            class="bucket-header px-3 py-1.5 flex items-center gap-2 text-left bg-surface-alt hover:bg-surface-hover transition-colors sticky"
            :class="bIdx > 0 ? 'border-t border-default' : 'border-t border-default'"
            :style="{ gridColumn: '1 / -1' }"
            @click="emit('toggle-bucket', bucket.key)"
          >
            <svg
              class="w-3 h-3 text-tertiary transition-transform"
              :class="isCollapsed?.(bucket.key) ? '-rotate-90' : ''"
              viewBox="0 0 12 12"
              fill="currentColor"
              aria-hidden="true"
            >
              <path d="M2 4l4 4 4-4z" />
            </svg>
            <span class="text-xs font-medium text-primary">{{ bucket.label }}</span>
            <span class="text-[10px] text-tertiary tabular-nums">{{ bucket.items.length }}</span>
          </button>

          <template v-if="!isCollapsed?.(bucket.key)">
            <template
              v-for="(item, index) in bucket.items"
              :key="`${bucket.key}:${String(fieldValue(item, itemIdField))}`"
            >
              <div
                class="contents group cursor-pointer"
                @click="emit('row-click', item)"
                @mouseenter="emit('row-mouseenter', item)"
              >
                <div
                  class="px-4 py-3 flex items-center bg-app group-hover:bg-surface-hover"
                  :class="[
                    loading ? 'opacity-60 pointer-events-none' : 'transition-colors',
                    index > 0 ? 'border-t border-default' : ''
                  ]"
                  @click.stop
                >
                  <Checkbox
                    :model-value="selectedItems.includes(String(fieldValue(item, itemIdField)))"
                    @change="(e) => emit('toggle-selection', e, String(fieldValue(item, itemIdField)))"
                  />
                </div>
                <div
                  v-for="column in columns"
                  :key="column.field"
                  :class="[
                    'px-2 py-3 flex items-center bg-app group-hover:bg-surface-hover text-sm min-w-0',
                    getColumnVisibility(column),
                    loading ? 'opacity-60 pointer-events-none' : 'transition-colors',
                    index > 0 ? 'border-t border-default' : ''
                  ]"
                >
                  <slot
                    :name="`cell-${column.field}`"
                    :item="item"
                    :value="fieldValue(item, column.field)"
                    :index="index"
                    :column="column"
                  >
                    <span class="truncate text-primary">
                      {{ fieldValue(item, column.field) }}
                    </span>
                  </slot>
                </div>
              </div>
            </template>
          </template>
        </template>
      </template>
    </div>
  </div>
</template>

<style scoped>
/* Responsive data table grid using CSS custom properties */
.data-table-grid {
  display: grid;
  grid-template-columns: var(--grid-cols-base);
  grid-auto-rows: max-content;
}

@media (min-width: 768px) {
  .data-table-grid {
    grid-template-columns: var(--grid-cols-md);
  }
}

@media (min-width: 1024px) {
  .data-table-grid {
    grid-template-columns: var(--grid-cols-lg);
  }
}

/* Custom scrollbar styling */
.overflow-y-auto::-webkit-scrollbar,
.overflow-x-auto::-webkit-scrollbar {
  width: 8px;
  height: 8px;
}

.overflow-y-auto::-webkit-scrollbar-track,
.overflow-x-auto::-webkit-scrollbar-track {
  background: var(--color-bg-app);
}

.overflow-y-auto::-webkit-scrollbar-thumb,
.overflow-x-auto::-webkit-scrollbar-thumb {
  background: var(--color-border-default);
  border-radius: 4px;
}

.overflow-y-auto::-webkit-scrollbar-thumb:hover,
.overflow-x-auto::-webkit-scrollbar-thumb:hover {
  background: var(--color-border-strong);
}

.overflow-x-auto::-webkit-scrollbar-corner {
  background: var(--color-bg-app);
}
</style> 