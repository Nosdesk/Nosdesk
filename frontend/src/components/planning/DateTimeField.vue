<script setup lang="ts">
/**
 * A date with an optional time beside it, for the due date and the start
 * date. The date is the app's DatePicker, unchanged. Once a date is set,
 * "Add time" appears beside it and gives way to the time once there is
 * one ("2:30 pm"); selecting either opens the time field. It adds no row.
 * A time cannot outlive its date: clearing the date clears both (the
 * server does, and the stores mirror it).
 */
import { computed, ref } from 'vue'
import DatePicker from '@/components/common/DatePicker.vue'
import IconButton from '@/components/common/IconButton.vue'
import type { PopoverAnchor } from '@/composables/popoverAnchor'
import { formatWallTime } from '@nosdesk/core/utils/ticketPlan'
import TimePopover from './TimePopover.vue'

const props = defineProps<{
  /** `YYYY-MM-DD`, or empty. */
  date: string
  /** `HH:MM:SS`, or null. */
  time: string | null | undefined
  label: string
  timeTitle: string
  /** Label of the clear button; without it there is none. */
  clearLabel?: string
  disabled?: boolean
}>()

const emit = defineEmits<{
  (e: 'update:date', day: string | null): void
  (e: 'update:time', time: string | null): void
}>()

const open = ref(false)
const trigger = ref<HTMLElement | null>(null)
const anchor: PopoverAnchor = { type: 'element', element: () => trigger.value }

const dateValue = computed<string>({
  get: () => props.date,
  set: (value) => emit('update:date', value || null),
})
</script>

<template>
  <div class="flex flex-wrap items-center gap-1" data-date-time>
    <!-- The date takes the line and the time and clear follow it, until
         the date would have less than eight and a half of the text's ems:
         then they wrap under it together. -->
    <DatePicker v-model="dateValue" :disabled="disabled" size="sm" block class="grow basis-[calc(var(--text-sm)*8.5)]" :aria-label="label" />
    <button
      v-if="date && !(disabled && !time)"
      ref="trigger"
      type="button"
      :disabled="disabled"
      class="min-h-7 shrink-0 rounded-md px-2 text-xs whitespace-nowrap tabular-nums transition-colors enabled:hover:bg-surface-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
      :class="time ? 'text-primary' : 'text-tertiary hover:text-primary'"
      aria-haspopup="dialog"
      :aria-expanded="open"
      :aria-label="time ? `${timeTitle}: ${formatWallTime(time)}` : undefined"
      @click="open = !open"
    >{{ time ? formatWallTime(time) : $t('plan-time-add') }}</button>
    <IconButton
      v-if="date && clearLabel && !disabled"
      size="sm"
      icon="close"
      :label="clearLabel"
      @click="emit('update:date', null)"
    />
    <TimePopover
      :open="open && !disabled"
      :anchor="anchor"
      :time="time"
      :title="timeTitle"
      placement="bottom-end"
      @update="(value) => emit('update:time', value)"
      @close="open = false"
    />
  </div>
</template>
