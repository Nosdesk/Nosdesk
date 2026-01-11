<script setup lang="ts">
/**
 * EmptyState Component
 *
 * A consistent, professional empty state display for when no items are found.
 * Supports multiple variants for different contexts (page, card, compact).
 * Uses responsive flex design with proper gap spacing.
 */

defineProps<{
  icon?: 'folder' | 'document' | 'users' | 'device' | 'ticket' | 'search' | 'inbox' | 'calendar' | 'key'
  title: string
  description?: string
  actionLabel?: string
  /** Variant controls the visual style:
   * - 'page': Full page empty state with larger icon and more padding (default)
   * - 'card': For use within cards/sections, medium sizing
   * - 'compact': Minimal version for modals/dropdowns
   */
  variant?: 'page' | 'card' | 'compact'
}>()

const emit = defineEmits<{
  action: []
}>()

const icons = {
  folder: 'M4 4h4v16H4V4zm6 0h4v12h-4V4zm6 0h4v8h-4V4z',
  document: 'M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z',
  users: 'M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z',
  device: 'M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z',
  ticket: 'M15 5v2m0 4v2m0 4v2M5 5a2 2 0 00-2 2v3a2 2 0 110 4v3a2 2 0 002 2h14a2 2 0 002-2v-3a2 2 0 110-4V7a2 2 0 00-2-2H5z',
  search: 'M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z',
  inbox: 'M20 13V6a2 2 0 00-2-2H6a2 2 0 00-2 2v7m16 0v5a2 2 0 01-2 2H6a2 2 0 01-2-2v-5m16 0h-2.586a1 1 0 00-.707.293l-2.414 2.414a1 1 0 01-.707.293h-3.172a1 1 0 01-.707-.293l-2.414-2.414A1 1 0 006.586 13H4',
  calendar: 'M8 7V3m8 4V3m-9 8h10M5 21h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z',
  key: 'M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z'
}
</script>

<template>
  <div
    class="flex flex-col items-center justify-center text-center"
    :class="{
      'py-16 px-6 gap-5': variant === 'page' || !variant,
      'py-10 px-4 gap-4': variant === 'card',
      'py-6 px-3 gap-3': variant === 'compact'
    }"
  >
    <!-- Icon container -->
    <div
      class="rounded-full flex items-center justify-center bg-surface-alt flex-shrink-0"
      :class="{
        'w-20 h-20': variant === 'page' || !variant,
        'w-16 h-16': variant === 'card',
        'w-12 h-12': variant === 'compact'
      }"
    >
      <svg
        class="text-tertiary"
        :class="{
          'w-10 h-10': variant === 'page' || !variant,
          'w-8 h-8': variant === 'card',
          'w-6 h-6': variant === 'compact'
        }"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
        stroke-width="1.5"
      >
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          :d="icons[icon || 'document']"
        />
      </svg>
    </div>

    <!-- Text content wrapper -->
    <div
      class="flex flex-col items-center"
      :class="{
        'gap-2': variant === 'page' || !variant,
        'gap-1.5': variant === 'card',
        'gap-1': variant === 'compact'
      }"
    >
      <!-- Title -->
      <h3
        class="font-medium text-secondary"
        :class="{
          'text-lg': variant === 'page' || !variant,
          'text-base': variant === 'card',
          'text-sm': variant === 'compact'
        }"
      >
        {{ title }}
      </h3>

      <!-- Description -->
      <p
        v-if="description"
        class="text-tertiary max-w-sm"
        :class="{
          'text-sm': variant === 'page' || !variant || variant === 'card',
          'text-xs': variant === 'compact'
        }"
      >
        {{ description }}
      </p>
    </div>

    <!-- Action button -->
    <button
      v-if="actionLabel"
      @click="emit('action')"
      class="font-medium text-white bg-accent rounded-lg hover:bg-accent-hover focus:ring-2 focus:outline-none focus:ring-accent/50 flex items-center justify-center gap-2 transition-colors"
      :class="{
        'px-5 py-2.5 text-sm min-w-[140px]': variant === 'page' || !variant,
        'px-4 py-2 text-sm min-w-[120px]': variant === 'card',
        'px-3 py-1.5 text-xs': variant === 'compact'
      }"
    >
      <svg
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
        :class="{
          'w-5 h-5': variant === 'page' || !variant,
          'w-4 h-4': variant === 'card' || variant === 'compact'
        }"
      >
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
      </svg>
      {{ actionLabel }}
    </button>
  </div>
</template>
