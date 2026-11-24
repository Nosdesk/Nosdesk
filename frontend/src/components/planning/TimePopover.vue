<script setup lang="ts">
/**
 * The time of a job: one time field in 15-minute steps, and Clear. No end
 * time; the length is the size. A popover from `md`, a bottom sheet below
 * it. The field commits when focus leaves it, on Enter and when the
 * surface closes, so closing is how a typed time is kept.
 */
import { computed } from 'vue'
import Button from '@/components/common/Button.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import TimePicker from '@/components/common/TimePicker.vue'
import type { PopoverAnchor, PopoverPlacement } from '@/composables/popoverAnchor'

const props = withDefaults(
  defineProps<{
    open: boolean
    anchor: PopoverAnchor
    /** `HH:MM:SS`, or null when the job has no time. */
    time: string | null | undefined
    title: string
    placement?: PopoverPlacement
  }>(),
  { placement: 'bottom-start' },
)

const emit = defineEmits<{
  /** `HH:MM:SS`, or null to clear. */
  (e: 'update', time: string | null): void
  (e: 'close'): void
}>()

const hhmm = computed(() => (props.time ? props.time.slice(0, 5) : ''))

function onUpdate(value: string): void {
  emit('update', value ? `${value}:00` : null)
}

/** Leaving the field is what commits a typed time, so leave it before
 *  the surface goes: the parent then hears the time before the close. */
function close(): void {
  const active = document.activeElement
  if (active instanceof HTMLElement && active.closest('[data-time-popover]')) active.blur()
  emit('close')
}

function clear(): void {
  emit('update', null)
  emit('close')
}
</script>

<template>
  <ResponsiveMenu
    :open="open"
    :anchor="anchor"
    :title="title"
    :aria-label="title"
    :placement="placement"
    role="dialog"
    popover-class="bg-surface border border-default rounded-lg shadow-lg"
    @close="close"
  >
    <form
      class="flex items-center gap-2 p-2 max-md:px-4 max-md:pb-4"
      data-time-popover
      @submit.prevent="close"
      @keydown.escape="close"
    >
      <TimePicker
        :model-value="hhmm"
        size="md"
        :minute-step="15"
        :aria-label="title"
        class="flex-1"
        @update:model-value="onUpdate"
        @keydown.enter="close"
      />
      <Button v-if="time" type="button" variant="ghost" size="sm" @click="clear">
        {{ $t('plan-time-clear') }}
      </Button>
    </form>
  </ResponsiveMenu>
</template>
