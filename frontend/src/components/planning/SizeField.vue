<script setup lang="ts">
/**
 * The Size field: the trigger the Status and Priority fields use
 * (CustomDropdown's), opening the size menu. Shows the word, not the
 * letter: there is room for it here.
 */
import { computed, ref } from 'vue'
import Icon from '@/components/common/Icon.vue'
import type { PopoverAnchor } from '@/composables/popoverAnchor'
import { sizeOf } from '@nosdesk/core/utils/ticketPlan'
import SizeMenu from './SizeMenu.vue'

const props = defineProps<{
  minutes: number | null | undefined
  /** Dense trigger for the preview pane's property rows. */
  compact?: boolean
  disabled?: boolean
}>()

const emit = defineEmits<{ (e: 'update', minutes: number | null): void }>()

const open = ref(false)
const trigger = ref<HTMLElement | null>(null)
const anchor: PopoverAnchor = { type: 'element', element: () => trigger.value }
const size = computed(() => sizeOf(props.minutes))
</script>

<template>
  <div class="relative" :class="compact ? 'inline-flex max-w-full' : 'w-full'" data-size-field>
    <button
      ref="trigger"
      type="button"
      aria-haspopup="menu"
      :aria-expanded="open"
      :disabled="disabled"
      class="group flex items-center justify-between bg-transparent text-left text-primary transition-colors enabled:hover:bg-surface-hover enabled:active:bg-surface-alt enabled:cursor-pointer disabled:cursor-default"
      :class="compact
        ? 'gap-1.5 rounded-md px-1.5 py-0.5 text-xs'
        : 'w-full rounded-lg px-3 py-2.5 sm:py-2 min-h-[44px] sm:min-h-[40px]'"
      @click="open = !open"
    >
      <span
        class="truncate"
        :class="[
          compact ? 'text-2xs leading-tight' : 'text-sm',
          size ? 'font-medium text-primary' : 'text-tertiary',
        ]"
      >{{ size ? $t(`plan-size-${size}`) : $t('plan-size-none') }}</span>
      <Icon
        v-if="!disabled"
        name="chevronDown"
        :size="compact ? 'xs' : 'sm'"
        class="shrink-0 text-tertiary transition-all duration-200"
        :class="[
          compact ? 'opacity-60 group-hover:opacity-100' : 'opacity-0 group-hover:opacity-100 pointer-coarse:opacity-100',
          { 'rotate-180 opacity-100': open },
        ]"
      />
    </button>
    <SizeMenu
      :open="open && !disabled"
      :anchor="anchor"
      :minutes="minutes"
      placement="bottom-start"
      :match-anchor-width="!compact"
      @select="(value) => emit('update', value)"
      @close="open = false"
    />
  </div>
</template>
