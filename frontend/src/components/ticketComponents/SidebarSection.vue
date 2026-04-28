<script setup lang="ts">
/**
 * SidebarSection - Section wrapper for ticket sidebar groups
 *
 * Provides the common section header + empty state pattern used for
 * Devices, Linked Tickets, Projects, and plugin panels in TicketView.
 */
import Icon from '@/components/common/Icon.vue';

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
        <Icon name="add" />
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
        <Icon name="add" />
        <span>{{ addLabel }}</span>
      </div>
    </button>
  </div>
</template>
