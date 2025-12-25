<script setup lang="ts">
import { computed } from 'vue'
import Checkbox from './Checkbox.vue'

interface Column {
  field: string
  label: string
  width?: string
  sortable?: boolean
  sortKey?: string // Optional different field name for API sorting (defaults to field)
  responsive?: 'always' | 'md' | 'lg' // Show only on certain breakpoints
}

interface DataTableProps {
  columns: Column[]
  data: any[]
  selectedItems: string[]
  itemIdField?: string
  sortField?: string
  sortDirection?: 'asc' | 'desc'
  loading?: boolean
  gridClass?: string // Custom grid template classes
}

const props = withDefaults(defineProps<DataTableProps>(), {
  itemIdField: 'id',
  loading: false,
  gridClass: ''
})

const emit = defineEmits<{
  'update:sort': [field: string, direction: 'asc' | 'desc']
  'toggle-selection': [event: Event, itemId: string]
  'toggle-all': [event: Event]
  'row-click': [item: any]
}>()

// Compute if all items are selected
const allSelected = computed(() => {
  if (!props.data.length) return false
  return props.data.every(item => 
    props.selectedItems.includes(item[props.itemIdField].toString())
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
        <!-- Checkbox Header -->
        <div class="px-4 py-3 flex items-center font-semibold text-primary bg-surface border-b-1 border-default sticky top-0 z-10">
          <Checkbox
            :model-value="allSelected && data.length > 0"
            @change="(e) => emit('toggle-all', e)"
          />
        </div>

        <!-- Column Headers -->
        <div
          v-for="column in columns"
          :key="column.field"
          :class="[
            'px-2 py-3 flex items-center font-semibold text-primary bg-surface border-b-1 border-default sticky top-0 z-10',
            getColumnVisibility(column),
            column.sortable ? 'cursor-pointer hover:bg-surface-hover' : ''
          ]"
          @click="toggleSort(column)"
        >
          <div class="flex items-center gap-1">
            {{ column.label }}
            <span v-if="column.sortable && isColumnSorted(column)" class="text-primary">
              {{ sortDirection === 'asc' ? '↑' : '↓' }}
            </span>
          </div>
        </div>
      </div>

      <!-- Data Rows -->
      <template v-for="(item, index) in data" :key="item[itemIdField]">
        <div
          class="contents group cursor-pointer"
          @click="emit('row-click', item)"
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
              :model-value="selectedItems.includes(item[itemIdField].toString())"
              @change="(e) => emit('toggle-selection', e, item[itemIdField].toString())"
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
            <!-- Slot for custom cell content -->
            <slot
              :name="`cell-${column.field}`"
              :item="item"
              :value="item[column.field]"
              :index="index"
              :column="column"
            >
              <!-- Default cell content -->
              <span class="truncate text-primary">
                {{ item[column.field] }}
              </span>
            </slot>
          </div>
        </div>
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