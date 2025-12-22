<script setup lang="ts">
import { computed } from 'vue'

export interface BulkAction {
  id: string
  label: string
  icon?: string
  variant?: 'default' | 'danger' | 'primary'
  /** If true, show confirmation dialog before executing */
  confirm?: boolean
  confirmMessage?: string
}

const props = defineProps<{
  selectedCount: number
  totalCount: number
  actions: BulkAction[]
  /** Item type label for display (e.g., "ticket", "device") */
  itemLabel?: string
}>()

const emit = defineEmits<{
  'action': [actionId: string]
  'clear-selection': []
  'select-all': []
}>()

const itemLabel = computed(() => props.itemLabel || 'item')
const pluralLabel = computed(() => props.selectedCount === 1 ? itemLabel.value : `${itemLabel.value}s`)

const getActionClasses = (action: BulkAction) => {
  const base = 'flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg transition-colors whitespace-nowrap'

  switch (action.variant) {
    case 'danger':
      return `${base} text-status-error hover:bg-status-error/10`
    case 'primary':
      return `${base} bg-accent text-white hover:opacity-90`
    default:
      return `${base} text-secondary hover:text-primary hover:bg-surface-hover`
  }
}

const handleAction = (action: BulkAction) => {
  if (action.confirm) {
    const message = action.confirmMessage || `Are you sure you want to ${action.label.toLowerCase()} ${props.selectedCount} ${pluralLabel.value}?`
    if (!confirm(message)) return
  }
  emit('action', action.id)
}
</script>

<template>
  <Transition
    enter-active-class="transition-all duration-200 ease-out"
    enter-from-class="opacity-0 translate-y-2"
    enter-to-class="opacity-100 translate-y-0"
    leave-active-class="transition-all duration-150 ease-in"
    leave-from-class="opacity-100 translate-y-0"
    leave-to-class="opacity-0 translate-y-2"
  >
    <div
      v-if="selectedCount > 0"
      class="sticky top-0 z-30 bg-surface border-b border-default shadow-md"
    >
      <div class="flex items-center justify-between gap-4 px-4 py-2">
        <!-- Left: Selection info -->
        <div class="flex items-center gap-3">
          <!-- Selection count -->
          <div class="flex items-center gap-2">
            <div class="flex items-center justify-center w-6 h-6 bg-accent text-white text-xs font-bold rounded-full">
              {{ selectedCount }}
            </div>
            <span class="text-sm text-secondary">
              {{ pluralLabel }} selected
            </span>
          </div>

          <!-- Select all / Clear -->
          <div class="flex items-center gap-1 text-xs">
            <button
              v-if="selectedCount < totalCount"
              @click="emit('select-all')"
              class="text-accent hover:underline"
            >
              Select all {{ totalCount }}
            </button>
            <span v-if="selectedCount < totalCount" class="text-tertiary">·</span>
            <button
              @click="emit('clear-selection')"
              class="text-tertiary hover:text-secondary"
            >
              Clear
            </button>
          </div>
        </div>

        <!-- Right: Actions -->
        <div class="flex items-center gap-1">
          <button
            v-for="action in actions"
            :key="action.id"
            @click="handleAction(action)"
            :class="getActionClasses(action)"
          >
            <!-- Delete icon -->
            <svg v-if="action.icon === 'delete'" class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
            </svg>
            <!-- Merge icon -->
            <svg v-else-if="action.icon === 'merge'" class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 7h12m0 0l-4-4m4 4l-4 4m0 6H4m0 0l4 4m-4-4l4-4" />
            </svg>
            <!-- Edit icon -->
            <svg v-else-if="action.icon === 'edit'" class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
            </svg>
            <!-- Status icon -->
            <svg v-else-if="action.icon === 'status'" class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <!-- Assign icon -->
            <svg v-else-if="action.icon === 'assign'" class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />
            </svg>
            <!-- Export icon -->
            <svg v-else-if="action.icon === 'export'" class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-8l-4-4m0 0L8 8m4-4v12" />
            </svg>
            <!-- Tag icon -->
            <svg v-else-if="action.icon === 'tag'" class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 7h.01M7 3h5c.512 0 1.024.195 1.414.586l7 7a2 2 0 010 2.828l-7 7a2 2 0 01-2.828 0l-7-7A1.994 1.994 0 013 12V7a4 4 0 014-4z" />
            </svg>
            {{ action.label }}
          </button>
        </div>
      </div>
    </div>
  </Transition>
</template>
