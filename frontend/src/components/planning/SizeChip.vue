<script setup lang="ts">
/**
 * A job's size on a row: one letter in a square, so the letters stack
 * into a column down the right edge of a list. The square is cut in em
 * with 20px as its least, so the letter grows with the text size
 * without leaving its box. An unsized job
 * shows nothing at rest and a dashed square on hover (`.plan-size-empty`
 * in main.css): sizing is offered, never nagged. Selecting it opens the
 * size menu.
 */
import { computed, ref } from 'vue'
import type { PopoverAnchor } from '@/composables/popoverAnchor'
import { sizeOf } from '@nosdesk/core/utils/ticketPlan'
import SizeMenu from './SizeMenu.vue'

const props = defineProps<{
  minutes: number | null | undefined
  number: number
}>()

const emit = defineEmits<{ (e: 'update', minutes: number | null): void }>()

const el = ref<HTMLButtonElement | null>(null)
const open = ref(false)
const anchor: PopoverAnchor = { type: 'element', element: () => el.value }
const size = computed(() => sizeOf(props.minutes))
</script>

<template>
  <button
    ref="el"
    type="button"
    class="inline-flex h-[max(1.25rem,1.7em)] w-[max(1.25rem,1.7em)] shrink-0 items-center justify-center rounded text-2xs font-medium leading-none transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
    :class="size
      ? 'border border-default text-secondary hover:border-strong hover:text-primary'
      : 'plan-size-empty border border-dashed border-strong text-tertiary'"
    aria-haspopup="menu"
    :aria-expanded="open"
    :aria-label="size
      ? $t('plan-size-aria', { number: String(number), size: $t(`plan-size-${size}`) })
      : $t('plan-size-set-aria', { number: String(number) })"
    :title="size ? $t(`plan-size-${size}`) : $t('plan-size-label')"
    data-no-drag
    @pointerdown.stop
    @click.stop="open = !open"
    @dblclick.stop
  >
    <template v-if="size">{{ $t(`plan-size-letter-${size}`) }}</template>
  </button>
  <SizeMenu
    :open="open"
    :anchor="anchor"
    :minutes="minutes"
    @select="(value) => emit('update', value)"
    @close="open = false"
  />
</template>
