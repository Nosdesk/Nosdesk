<script setup lang="ts">
import { computed } from 'vue'

interface Column {
  field: string
  label: string
  width?: string
  sortable?: boolean
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
const toggleSort = (field: string) => {
  if (!props.columns.find(col => col.field === field)?.sortable) return
  
  if (props.sortField === field) {
    const newDirection = props.sortDirection === 'asc' ? 'desc' : 'asc'
    emit('update:sort', field, newDirection)
  } else {
    emit('update:sort', field, 'asc')
  }
}

// Generate responsive grid classes
const gridClasses = computed(() => {
  if (props.gridClass) return props.gridClass
  
  // Default responsive grid based on column count and responsive settings
  const baseColumns = props.columns.filter(col => !col.responsive || col.responsive === 'always')
  const mdColumns = props.columns.filter(col => !col.responsive || col.responsive === 'always' || col.responsive === 'md')
  const lgColumns = props.columns.filter(col => !col.responsive || col.responsive === 'always' || col.responsive === 'md' || col.responsive === 'lg')
  
  const baseGrid = `grid-cols-[auto_${baseColumns.map(col => col.width || '1fr').join('_')}]`
  const mdGrid = `md:grid-cols-[auto_${mdColumns.map(col => col.width || '1fr').join('_')}]`
  const lgGrid = `lg:grid-cols-[auto_${lgColumns.map(col => col.width || '1fr').join('_')}]`
  
  return `${baseGrid} ${mdGrid} ${lgGrid}`
})

// Helper to determine if column should be visible at current breakpoint
const getColumnVisibility = (column: Column) => {
  if (!column.responsive || column.responsive === 'always') return ''
  if (column.responsive === 'md') return 'hidden md:flex'
  if (column.responsive === 'lg') return 'hidden lg:flex'
  return ''
}
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Grid Container -->
    <div :class="['grid auto-rows-max', gridClasses]">
      
      <!-- Sticky Header Row -->
      <div class="contents sticky top-0 z-10">
        <!-- Checkbox Header -->
        <div class="px-4 py-3 flex items-center font-semibold text-primary bg-surface border-b-1 border-default sticky top-0 z-10">
          <input
            type="checkbox"
            class="w-4 h-4 rounded border-default bg-surface text-brand-blue focus:ring-brand-blue"
            :checked="allSelected && data.length > 0"
            @click="emit('toggle-all', $event)"
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
          @click="toggleSort(column.field)"
        >
          <div class="flex items-center gap-1">
            {{ column.label }}
            <span v-if="column.sortable && sortField === column.field" class="text-primary">
              {{ sortDirection === 'asc' ? '↑' : '↓' }}
            </span>
          </div>
        </div>
      </div>

      <!-- Data Rows -->
      <template v-for="(item, index) in data" :key="item[itemIdField]">
        <div class="contents group cursor-pointer" @click="emit('row-click', item)">
          <!-- Checkbox Cell -->
          <div class="px-4 py-3 flex items-center bg-app group-hover:bg-surface-hover transition-colors border-b border-default">
            <input
              type="checkbox"
              class="w-4 h-4 rounded border-default bg-surface text-brand-blue focus:ring-brand-blue"
              :checked="selectedItems.includes(item[itemIdField].toString())"
              @click.stop="emit('toggle-selection', $event, item[itemIdField].toString())"
            />
          </div>

          <!-- Data Cells -->
          <div
            v-for="column in columns"
            :key="column.field"
            :class="[
              'px-2 py-3 flex items-center bg-app group-hover:bg-surface-hover transition-colors text-sm border-b border-default min-w-0',
              getColumnVisibility(column)
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