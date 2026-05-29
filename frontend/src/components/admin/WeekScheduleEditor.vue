<script setup lang="ts">
/**
 * Visual editor for a working-calendar weekly schedule.
 *
 * The schedule is the same `Record<day, [open, close][]>` shape the
 * backend stores in JSONB: keys mon..sun, values are zero or more
 * [open, close] string pairs (HH:MM 24h, in the calendar's
 * timezone).
 *
 * Each day is a row carrying a `DayTimeline` — a 24h horizontal
 * track with draggable range bars. Drag the bar edges to resize,
 * drag the body to shift, click empty space to create. Direct
 * manipulation beats the prior text-input chip layout because the
 * admin can see the day shape at a glance and adjust without
 * mentally converting "09:00 → 17:00" into spatial intuition.
 *
 * Validation is render-only (close <= open shows as error tone);
 * the `update:valid` watcher lets the parent gate the Save button
 * so the admin sees the data refused to commit instead of vanishing
 * silently through the backend schedule parser.
 */
import { computed, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import DayTimeline from '@/components/admin/DayTimeline.vue'

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

export type DayKey = 'mon' | 'tue' | 'wed' | 'thu' | 'fri' | 'sat' | 'sun'
export type DaySchedule = [string, string][]
export type WeekSchedule = Record<DayKey, DaySchedule>

const DAYS: { key: DayKey; labelKey: string }[] = [
  { key: 'mon', labelKey: 'admin-sla-schedule-day-mon' },
  { key: 'tue', labelKey: 'admin-sla-schedule-day-tue' },
  { key: 'wed', labelKey: 'admin-sla-schedule-day-wed' },
  { key: 'thu', labelKey: 'admin-sla-schedule-day-thu' },
  { key: 'fri', labelKey: 'admin-sla-schedule-day-fri' },
  { key: 'sat', labelKey: 'admin-sla-schedule-day-sat' },
  { key: 'sun', labelKey: 'admin-sla-schedule-day-sun' },
]

const model = defineModel<WeekSchedule>({ required: true })

// Surfacing validity lets the parent's Save button stay disabled
// while any range is malformed.
const emit = defineEmits<{ (e: 'update:valid', valid: boolean): void }>()

function dayRanges(day: DayKey): DaySchedule {
  return model.value[day] ?? []
}

function setDayRanges(day: DayKey, ranges: DaySchedule): void {
  // Replace the whole row in a shallow clone so the parent ref's
  // reactivity flips for the watcher chain (sync_action emit, draft
  // dirty-check, etc.) without us having to mutate in place.
  model.value = { ...model.value, [day]: ranges }
}

function isValid(range: [string, string]): boolean {
  return range[0] < range[1]
}

const allRangesValid = computed(() =>
  DAYS.every((d) => dayRanges(d.key).every(isValid)),
)

watch(allRangesValid, (v) => emit('update:valid', v), { immediate: true })
</script>

<template>
  <div class="flex flex-col gap-1.5">
    <div
      v-for="day in DAYS"
      :key="day.key"
      class="flex items-center gap-3"
    >
      <span
        class="w-10 flex-shrink-0 text-[11px] font-semibold text-tertiary uppercase tracking-wide"
      >
        {{ $t(day.labelKey) }}
      </span>
      <DayTimeline
        class="flex-1 min-w-0"
        :ranges="dayRanges(day.key)"
        :aria-label="t(day.labelKey)"
        @update:ranges="(ranges: DaySchedule) => setDayRanges(day.key, ranges)"
      />
    </div>
    <p class="text-[10px] text-tertiary italic mt-1">
      {{ $t('admin-sla-schedule-timeline-hint') }}
    </p>
  </div>
</template>
