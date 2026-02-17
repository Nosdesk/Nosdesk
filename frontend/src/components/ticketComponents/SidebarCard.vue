<script setup lang="ts">
/**
 * SidebarCard - Shared card shell for ticket sidebar items
 *
 * Provides the common pattern used by DeviceDetails, LinkedTicketPreview,
 * ProjectInfo, and plugin sidebar panels:
 * - Card container with hover border
 * - Header with content slot + remove button
 * - Body content slot
 * - Print layout slot
 */

interface Props {
  /** Tooltip for the remove button */
  removeTitle?: string;
  /** Make the entire card clickable */
  clickable?: boolean;
  /** Disable the remove button */
  removeDisabled?: boolean;
}

withDefaults(defineProps<Props>(), {
  removeTitle: 'Remove',
  clickable: false,
  removeDisabled: false,
});

defineEmits<{
  (e: 'remove'): void;
  (e: 'click'): void;
}>();
</script>

<template>
  <!-- Screen layout -->
  <div
    class="print:hidden bg-surface rounded-xl border border-default overflow-hidden hover:border-strong transition-colors"
    :class="{ 'cursor-pointer': clickable }"
    @click="clickable && $emit('click')"
  >
    <!-- Header -->
    <div class="px-4 py-3 bg-surface-alt border-b border-default">
      <div class="flex items-center justify-between gap-2">
        <div class="flex items-center gap-3 min-w-0 flex-1">
          <slot name="header" />
        </div>
        <button
          @click.stop="$emit('remove')"
          :disabled="removeDisabled"
          class="p-1.5 flex-shrink-0 text-tertiary hover:text-status-error hover:bg-status-error/20 rounded-md transition-colors disabled:opacity-50"
          :title="removeTitle"
        >
          <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
            <path
              fill-rule="evenodd"
              d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z"
              clip-rule="evenodd"
            />
          </svg>
        </button>
      </div>
    </div>

    <!-- Body -->
    <div class="p-4">
      <slot />
    </div>
  </div>

  <!-- Print layout -->
  <slot name="print" />
</template>
