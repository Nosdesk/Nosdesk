<script setup lang="ts">
/**
 * SidebarSection - Section wrapper for ticket sidebar groups
 *
 * Provides the common section header + empty state pattern used for
 * Devices, Linked Tickets, Projects, and plugin panels in TicketView.
 */

interface Props {
  /** Section title */
  title: string;
  /** Label for the add action (e.g. "Add device") */
  addLabel: string;
  /** Whether items exist (controls header vs empty state display) */
  hasItems: boolean;
  /** Hide the entire section on print when empty */
  hideOnPrint?: boolean;
  /** Suppress the dashed empty-state button (unified menu handles it) */
  hideEmptyState?: boolean;
}

withDefaults(defineProps<Props>(), {
  hideOnPrint: false,
  hideEmptyState: false,
});

defineEmits<{
  (e: 'add'): void;
}>();
</script>

<template>
  <div
    class="flex flex-col gap-2"
    :class="{ 'print:hidden': hideOnPrint && !hasItems }"
  >
    <!-- Header with title and add action (shown when items exist) -->
    <div v-if="hasItems" class="flex items-center justify-between">
      <h3 class="text-sm font-medium text-secondary">{{ title }}</h3>
      <button
        @click="$emit('add')"
        class="print:hidden flex items-center gap-1 text-xs font-medium text-tertiary hover:text-accent transition-colors"
      >
        <svg class="w-3.5 h-3.5" viewBox="0 0 20 20" fill="currentColor">
          <path d="M10 3a1 1 0 011 1v5h5a1 1 0 110 2h-5v5a1 1 0 11-2 0v-5H4a1 1 0 110-2h5V4a1 1 0 011-1z" />
        </svg>
        {{ addLabel }}
      </button>
    </div>

    <!-- Items slot -->
    <slot />

    <!-- Empty state (shown when no items, unless unified menu handles it) -->
    <button
      v-if="!hasItems && !hideEmptyState"
      @click="$emit('add')"
      class="print:hidden group w-full py-3 px-4 rounded-xl border border-dashed border-default hover:border-accent/50 hover:bg-accent/5 transition-all duration-150 cursor-pointer"
    >
      <div class="flex items-center justify-center gap-2 text-sm text-tertiary group-hover:text-accent transition-colors">
        <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
          <path d="M10 3a1 1 0 011 1v5h5a1 1 0 110 2h-5v5a1 1 0 11-2 0v-5H4a1 1 0 110-2h5V4a1 1 0 011-1z" />
        </svg>
        <span>{{ addLabel }}</span>
      </div>
    </button>
  </div>
</template>
