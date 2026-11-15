<script setup lang="ts">
/**
 * EmptyState Component
 *
 * A consistent, professional empty state display for when no items are found.
 * Supports multiple variants for different contexts (page, card, compact).
 * Uses responsive flex design with proper gap spacing.
 */
import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'
import type { IconName } from '@/components/common/icons'

defineProps<{
  /** Registry icon for the hero glyph; defaults to `document`. */
  icon?: IconName
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
      <Icon
        :name="icon || 'document'"
        :size="variant === 'compact' ? 'lg' : 'xl'"
        class="text-tertiary"
      />
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

    <!-- Actions: a slot for more than one, else the single action button -->
    <slot v-if="$slots.actions" name="actions" />
    <Button
      v-else-if="actionLabel"
      :size="variant === 'compact' ? 'sm' : variant === 'card' ? 'md' : 'lg'"
      :class="{
        'min-w-[140px]': variant === 'page' || !variant,
        'min-w-[120px]': variant === 'card',
      }"
      icon="add"
      @click="emit('action')"
    >
      {{ actionLabel }}
    </Button>
  </div>
</template>
