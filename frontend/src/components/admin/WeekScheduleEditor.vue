<script setup lang="ts">
/**
 * Visual editor for a working-calendar weekly schedule.
 *
 * The schedule is the same `Record<day, [open, close][]>` shape the
 * backend stores in JSONB: keys mon..sun, values are zero or more
 * [open, close] string pairs (HH:MM 24h, in the calendar's
 * timezone). The editor never reshapes or sorts the data; it just
 * mutates a deep copy via v-model so the parent's existing
 * Pinia-backed reactivity flows straight back to the API.
 *
 * Each day is a row. Empty days show a ghost "+ Add hours"
 * affordance; days with ranges render each range as a chip carrying
 * two native `<input type="time">` controls plus a remove button.
 * The chip *is* the editor (no read/edit mode toggle, no popover) so
 * an admin can edit times with the same keystroke economy as a real
 * spreadsheet, and the whole component fits in one viewport.
 *
 * Validation is light on purpose: the schedule parser on the engine
 * side silently drops ranges where `close <= open`, so a typo
 * doesn't blow up — it just renders as "Closed" for the affected
 * day. UI mirrors that: invalid ranges show in tertiary tone instead
 * of accent.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'

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

// Default range for the "+ Add hours" button. Same shape engineers
// type by hand most often, and the schedule parser is happy with it.
const DEFAULT_RANGE: [string, string] = ['09:00', '17:00']

const model = defineModel<WeekSchedule>({ required: true })

function dayRanges(day: DayKey): DaySchedule {
  return model.value[day] ?? []
}

function setDay(day: DayKey, ranges: DaySchedule) {
  // Replace the whole row in a shallow clone so the parent ref's
  // reactivity flips for the watcher chain (sync_action emit, draft
  // dirty-check, etc.) without us having to mutate in place.
  model.value = { ...model.value, [day]: ranges }
}

function addRange(day: DayKey) {
  setDay(day, [...dayRanges(day), [...DEFAULT_RANGE]])
}

function updateRange(day: DayKey, index: number, which: 0 | 1, value: string) {
  const next = dayRanges(day).map((r, i) =>
    i === index ? ([which === 0 ? value : r[0], which === 1 ? value : r[1]] as [string, string]) : r,
  )
  setDay(day, next)
}

function removeRange(day: DayKey, index: number) {
  setDay(
    day,
    dayRanges(day).filter((_, i) => i !== index),
  )
}

/** Render-only check; invalid ranges keep their data but go muted. */
function isValid(range: [string, string]): boolean {
  return range[0] < range[1]
}

const weekIsEmpty = computed(() => DAYS.every((d) => dayRanges(d.key).length === 0))
</script>

<template>
  <div class="flex flex-col gap-2">
    <div
      v-for="day in DAYS"
      :key="day.key"
      class="flex items-start gap-3 py-1.5 border-b border-subtle last:border-b-0"
    >
      <span
        class="w-10 flex-shrink-0 text-xs font-medium text-tertiary uppercase tracking-wide pt-1.5"
      >
        {{ $t(day.labelKey) }}
      </span>
      <div class="flex-1 flex flex-wrap items-center gap-1.5 min-h-[28px]">
        <span
          v-for="(range, i) in dayRanges(day.key)"
          :key="i"
          class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md border text-xs tabular-nums transition-colors"
          :class="
            isValid(range)
              ? 'bg-accent-muted border-accent/30 text-primary'
              : 'bg-surface-alt border-subtle text-tertiary'
          "
        >
          <input
            type="time"
            :value="range[0]"
            :aria-label="t('admin-sla-schedule-open-aria')"
            class="bg-transparent border-0 p-0 text-xs tabular-nums focus:outline-none focus-visible:ring-1 focus-visible:ring-accent rounded"
            @input="
              (e) =>
                updateRange(day.key, i, 0, (e.target as HTMLInputElement).value)
            "
          />
          <span class="text-tertiary" aria-hidden="true">→</span>
          <input
            type="time"
            :value="range[1]"
            :aria-label="t('admin-sla-schedule-close-aria')"
            class="bg-transparent border-0 p-0 text-xs tabular-nums focus:outline-none focus-visible:ring-1 focus-visible:ring-accent rounded"
            @input="
              (e) =>
                updateRange(day.key, i, 1, (e.target as HTMLInputElement).value)
            "
          />
          <button
            type="button"
            class="ml-0.5 text-tertiary hover:text-status-error transition-colors"
            :aria-label="t('admin-sla-schedule-remove-range-aria')"
            @click="removeRange(day.key, i)"
          >
            <Icon name="close" class="w-3 h-3" />
          </button>
        </span>
        <button
          type="button"
          class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md border border-dashed border-subtle text-xs text-tertiary hover:text-accent hover:border-accent/40 transition-colors"
          @click="addRange(day.key)"
        >
          <Icon name="add" class="w-3 h-3" />
          <span v-if="dayRanges(day.key).length === 0">
            {{ $t('admin-sla-schedule-add-hours') }}
          </span>
          <span v-else class="sr-only">{{ $t('admin-sla-schedule-add-range-aria') }}</span>
        </button>
      </div>
    </div>
    <p v-if="weekIsEmpty" class="text-[11px] text-tertiary italic">
      {{ $t('admin-sla-schedule-empty-hint') }}
    </p>
  </div>
</template>
