<script setup lang="ts">
/**
 * The calendar view shape: jobs on the days they are planned for, read
 * as a month or as an agenda.
 *
 * One header line, the same as My day's: the month as the title, what is
 * on it and the way back to today beside the title, then the range and
 * the way to another month.
 *
 *   Month   takes the layout its own width allows, measured on the view
 *           and not the window, so it also holds beside an open preview
 *           pane. Wide, a day is a cell of job titles and a full cell
 *           opens as a list. Narrow, a day is a number and a load mark,
 *           and the selected day's jobs follow in the page's own flow.
 *   Agenda  the days that have something on them, as one list that opens
 *           on today. Where My day is one person's plan for one day, this
 *           is every job in the view, across days.
 *
 * A job sits on its plan day (`calendar.ts`). Today, now, the week's
 * first day and every date's wording come from the planning timezone and
 * the workspace locale, not the device. Everything derives from the
 * tickets already held, so the view paints at once and has no loading
 * state.
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'
import IconButton from '@/components/common/IconButton.vue'
import Popover from '@/components/common/Popover.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import TicketDragPreview from '@/components/common/TicketDragPreview.vue'
import type { PopoverAnchor } from '@/composables/popoverAnchor'
import { useWallClock } from '@/composables/useToday'
import { useDateStore } from '@nosdesk/core/stores/dateStore'
import { planningTimezone, type Day } from '@nosdesk/core/utils/ticketDates'
import { loadHours, loadMinutes } from '@nosdesk/core/utils/ticketPlan'
import {
  agendaRange,
  anchorDay,
  dayTitle,
  earliestLate,
  isLate,
  jobsByDay,
  jobsOn,
  monthOf,
  monthTitle,
  monthWeeks,
  movePatch,
  shiftMonth,
  type CalendarRange,
  type DateField,
  type DayJobs,
  type Month,
} from '@nosdesk/core/sync/views/calendar'
import type { CardData } from '@nosdesk/core/sync/views/types'
import AgendaList from './AgendaList.vue'
import type { CalendarOverlay } from './calendarTypes'
import DayList from './DayList.vue'
import MonthCompact from './MonthCompact.vue'
import MonthGrid from './MonthGrid.vue'
import { useDayDrag } from './useDayDrag'

const props = withDefaults(
  defineProps<{
    cards: readonly CardData[]
    /** Which field places a job on a day. */
    dateField?: DateField
    overlays?: readonly CalendarOverlay[]
  }>(),
  { dateField: 'due_date', overlays: () => [] },
)

const emit = defineEmits<{
  (e: 'open', id: number): void
  /** A plan write: a day move, a size. */
  (e: 'patch', id: number, patch: Partial<CardData>): void
  /** The days whose other dates (warranties and the like) are wanted. */
  (e: 'visible-range', range: { start: Day; end: Day }): void
}>()

/** Month or agenda. The tickets view keeps it with the view. */
const range = defineModel<CalendarRange>('range', { default: 'month' })

const fluent = useFluent()
const clock = useWallClock()
const dateStore = useDateStore()
const locale = computed(() => dateStore.locale)

const ranges = computed<{ value: CalendarRange; label: string }[]>(() => [
  { value: 'month', label: fluent.$t('calendar-range-month') },
  { value: 'agenda', label: fluent.$t('calendar-range-agenda') },
])

// --- The month ---------------------------------------------------------

const month = ref<Month>(monthOf(clock.value.day))
const weeks = computed(() => monthWeeks(month.value, locale.value))
const offToday = computed(() => month.value !== monthOf(clock.value.day))

/** The day in the tab order, and on a narrow screen the day shown. */
const focusDay = ref<Day>(clock.value.day)

function restIn(next: Month): Day {
  return next === monthOf(clock.value.day) ? clock.value.day : `${next}-01`
}

function turn(by: number): void {
  month.value = shiftMonth(month.value, by)
  focusDay.value = restIn(month.value)
  closeLists()
}

function goToday(): void {
  month.value = monthOf(clock.value.day)
  focusDay.value = clock.value.day
  closeLists()
}

/** The roving day moved; follow it into a neighbouring month. */
function moveTo(day: Day): void {
  focusDay.value = day
  if (monthOf(day) !== month.value) {
    month.value = monthOf(day)
    closeLists()
  }
}

watch(
  () => props.dateField,
  () => goToday(),
)

// --- What is on it -----------------------------------------------------

const timed = computed(() => props.dateField === 'due_date')
const byDay = computed(() => jobsByDay(props.cards, props.dateField, planningTimezone()))
const undated = computed<DayJobs>(() => jobsOn(byDay.value, null))

const overlaysByDay = computed(() => {
  const map = new Map<Day, CalendarOverlay[]>()
  for (const overlay of props.overlays) {
    const bucket = map.get(overlay.date)
    if (bucket) bucket.push(overlay)
    else map.set(overlay.date, [overlay])
  }
  return map
})

/** What the parent fetches other dates for: the weeks on screen, or the
 *  agenda's span. */
const visibleRange = computed(() => {
  if (range.value === 'agenda') return agendaRange(byDay.value.keys(), clock.value.day)
  const rows = weeks.value
  return { start: rows[0][0], end: rows[rows.length - 1][6] }
})
watch(
  () => `${visibleRange.value.start}..${visibleRange.value.end}`,
  () => emit('visible-range', visibleRange.value),
  { immediate: true },
)

const inMonth = computed(() =>
  props.cards.filter((card) => {
    const day = anchorDay(card, props.dateField)
    return day != null && monthOf(day) === month.value
  }),
)

// --- The agenda --------------------------------------------------------

const agenda = ref<InstanceType<typeof AgendaList> | null>(null)
/** The day at the top of the agenda, which names the month in the
 *  title, and whether today is on screen. */
const agendaTop = ref<Day>(clock.value.day)
const agendaOnToday = ref(true)

function onAgendaPosition(top: Day, todayInView: boolean): void {
  agendaTop.value = top
  agendaOnToday.value = todayInView
}

/** From today on: what the agenda opens on. */
const upcoming = computed(() =>
  props.cards.filter((card) => {
    const day = anchorDay(card, props.dateField)
    return day != null && day >= clock.value.day
  }),
)

const title = computed(() =>
  monthTitle(range.value === 'agenda' ? monthOf(agendaTop.value) : month.value, locale.value),
)

/** Late jobs counted in the header: the month's, or every one in the
 *  agenda, most of which sit above today. */
const lateCount = computed(() => {
  const cards = range.value === 'agenda' ? props.cards : inMonth.value
  return cards.filter((card) => isLate(card, clock.value)).length
})

const summary = computed(() => {
  const isAgenda = range.value === 'agenda'
  return {
    count: isAgenda ? upcoming.value.length : inMonth.value.length,
    empty: fluent.$t(isAgenda ? 'calendar-agenda-empty' : 'calendar-month-empty'),
  }
})

watch(range, () => {
  closeLists()
  agendaTop.value = clock.value.day
  agendaOnToday.value = true
})

function load(jobs: readonly CardData[]): string {
  const minutes = loadMinutes(jobs)
  return minutes > 0
    ? fluent.$t('plan-day-load-sized', { count: jobs.length, hours: loadHours(minutes) })
    : fluent.$t('plan-day-load', { count: jobs.length })
}

/** What a screen reader hears for a day. */
function dayLabel(day: Day): string {
  const jobs = jobsOn(byDay.value, day).jobs
  const parts = [dayTitle(day, locale.value)]
  if (day === clock.value.day) parts.push(fluent.$t('calendar-today'))
  if (jobs.length) parts.push(fluent.$t('plan-day-load', { count: jobs.length }))
  const late = jobs.filter((card) => isLate(card, clock.value)).length
  if (late) parts.push(fluent.$t('calendar-overdue-count', { count: late }))
  const dates = overlaysByDay.value.get(day)?.length ?? 0
  if (dates) parts.push(fluent.$t('calendar-overlay-count', { count: dates }))
  return parts.join(', ')
}

// --- Which layout ------------------------------------------------------

// A cell needs about 96px to hold a time and the start of a title.
const WIDE_FROM = 672

const root = ref<HTMLElement | null>(null)
const narrow = ref(false)
let observer: ResizeObserver | null = null

onMounted(() => {
  const measure = () => {
    if (root.value && root.value.clientWidth) narrow.value = root.value.clientWidth < WIDE_FROM
  }
  measure()
  if (typeof ResizeObserver === 'undefined') return
  observer = new ResizeObserver(measure)
  if (root.value) observer.observe(root.value)
})
onBeforeUnmount(() => observer?.disconnect())

watch(narrow, () => closeLists())

// --- A day's list (wide) -----------------------------------------------

const listDay = ref<Day | null>(null)
const listCell = ref<HTMLElement | null>(null)
/** The list opens over its cell, from the cell's own top left corner,
 *  and the popover keeps itself on screen from there. */
const listOffset = ref(0)
const dayAnchor: PopoverAnchor = { type: 'element', element: () => listCell.value }
const listJobs = computed(() => jobsOn(byDay.value, listDay.value))
const listOverlays = computed(() => (listDay.value ? (overlaysByDay.value.get(listDay.value) ?? []) : []))
/** The row the list puts the focus on as it opens, in place of its
 *  first control. */
const focusRow = ref<number | null>(null)

function openDay(day: Day, cell: HTMLElement): void {
  if (listDay.value === day) {
    listDay.value = null
    return
  }
  // A day with nothing on it has nothing to open.
  if (!jobsOn(byDay.value, day).jobs.length && !overlaysByDay.value.get(day)?.length) return
  undatedOpen.value = false
  focusRow.value = null
  listCell.value = cell
  listOffset.value = -cell.offsetHeight
  listDay.value = day
}

// --- Jobs with no date -------------------------------------------------

const undatedOpen = ref(false)
const undatedButton = ref<HTMLElement | null>(null)
const undatedAnchor: PopoverAnchor = { type: 'element', element: () => undatedButton.value }

function closeLists(): void {
  listDay.value = null
  undatedOpen.value = false
}

// --- The way to what is late -------------------------------------------

/** Put the focus on a row once it is on screen, wherever it renders. */
function focusRowWhenShown(id: number, tries = 10): void {
  requestAnimationFrame(() => {
    const target = document.querySelector<HTMLElement>(`[data-calendar-rows] [data-row="${id}"] [data-row-open]`)
    if (target) {
      target.scrollIntoView({ block: 'nearest' })
      target.focus({ preventScroll: true })
    } else if (tries > 0) {
      focusRowWhenShown(id, tries - 1)
    }
  })
}

/** The overdue count goes to the earliest late job: in the agenda its
 *  row, in the month its day's list. */
async function goLate(): Promise<void> {
  if (range.value === 'agenda') {
    const target = earliestLate(byDay.value, clock.value)
    if (target) await agenda.value?.reveal(target.day, target.card.id)
    return
  }
  const target = earliestLate(byDay.value, clock.value, (day) => monthOf(day) === month.value)
  if (!target) return
  focusDay.value = target.day
  closeLists()
  if (narrow.value) {
    await nextTick()
    focusRowWhenShown(target.card.id)
    return
  }
  await nextTick()
  const cell = root.value?.querySelector<HTMLElement>(`[data-month-grid] [data-day="${target.day}"]`)
  if (!cell) return
  openDay(target.day, cell)
  focusRow.value = target.card.id
  focusRowWhenShown(target.card.id)
}

// --- Carrying a job to another day -------------------------------------

/** The move writes the date that supplies the plan day and leaves the
 *  time alone. */
const drag = useDayDrag((id, day) => {
  const card = props.cards.find((c) => c.id === id)
  const patch = card ? movePatch(card, day) : null
  if (patch) emit('patch', id, patch)
})
const dragged = computed(() => props.cards.find((c) => c.id === drag.draggedId.value) ?? null)

function onItemDown(id: number, event: PointerEvent): void {
  if (timed.value) drag.onItemDown(id, event)
}

function open(id: number): void {
  if (drag.justDragged.value) return
  closeLists()
  emit('open', id)
}

function patch(id: number, value: Partial<CardData>): void {
  emit('patch', id, value)
}

const POPOVER = 'flex w-[22.5rem] max-w-[calc(100vw-1rem)] max-h-[min(32rem,calc(100dvh-2rem))] flex-col overflow-hidden rounded-xl border border-default bg-surface'
</script>

<template>
  <div ref="root" class="flex min-h-0 min-w-0 flex-col bg-surface" data-calendar :data-layout="narrow ? 'narrow' : 'wide'">
    <!-- The month, what is on it, and the way to another month. -->
    <header class="flex shrink-0 items-start gap-2 px-4 pt-4 pb-2">
      <div class="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-3">
        <h2 class="text-lg leading-7 font-semibold text-primary" aria-live="polite">{{ title }}</h2>
        <p class="text-xs leading-5 text-secondary tabular-nums" data-month-load>
          <template v-if="summary.count">{{ $t(range === 'agenda' ? 'calendar-agenda-load' : 'plan-day-load', { count: summary.count }) }}</template>
          <template v-else>{{ summary.empty }}</template>
          <template v-if="lateCount">, <button
            type="button"
            class="-mx-0.5 rounded px-0.5 hover:underline focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            :aria-label="$t('calendar-overdue-jump', { count: lateCount })"
            data-overdue
            @click="goLate"
          ><span class="text-rose-600 dark:text-rose-400">{{ $t('calendar-overdue-count', { count: lateCount }) }}</span></button></template>
        </p>
        <!-- The way back, beside what it leaves: it wraps under the
             title with the summary where the line is short, so the
             controls on the right never grow. -->
        <button
          v-if="range === 'agenda' ? !agendaOnToday : offToday"
          type="button"
          class="-mx-1.5 -my-1 rounded-md px-1.5 py-1 text-xs leading-5 font-medium text-primary transition-colors hover:bg-surface-hover"
          data-today
          @click="range === 'agenda' ? agenda?.goToday() : goToday()"
        >{{ $t('calendar-today') }}</button>
      </div>
      <div class="flex h-7 shrink-0 items-center gap-0.5">
        <button
          v-if="!narrow && undated.jobs.length"
          ref="undatedButton"
          type="button"
          class="h-7 rounded-md px-2 text-xs text-secondary tabular-nums transition-colors hover:bg-surface-hover hover:text-primary"
          :class="undatedOpen && 'bg-surface-hover text-primary'"
          aria-haspopup="dialog"
          :aria-expanded="undatedOpen"
          data-undated-button
          @click="(undatedOpen = !undatedOpen), (listDay = null)"
        >{{ $t('calendar-undated-button', { count: undated.jobs.length }) }}</button>
        <SegmentedControl
          v-model="range"
          :options="ranges"
          size="sm"
          :aria-label="$t('calendar-range-label')"
          data-range
        />
        <!-- The agenda is one list: scrolling is how it moves. -->
        <template v-if="range === 'month'">
          <IconButton size="sm" icon="chevronLeft" :label="$t('calendar-previous-month')" @click="turn(-1)" />
          <IconButton size="sm" icon="chevronRight" :label="$t('calendar-next-month')" @click="turn(1)" />
        </template>
      </div>
    </header>

    <AgendaList
      v-if="range === 'agenda'"
      ref="agenda"
      data-calendar-rows
      :by-day="byDay"
      :overlays-by-day="overlaysByDay"
      :clock="clock"
      :locale="locale"
      :timed="timed"
      :drop-day="drag.dropDay.value"
      :dragged-id="drag.draggedId.value"
      @open="open"
      @patch="patch"
      @row-down="onItemDown"
      @position="onAgendaPosition"
    >
      <!-- Narrow: the jobs with no date close the list, as they close
           the month's page. -->
      <template v-if="narrow && undated.jobs.length" #after>
        <section class="flex flex-col" data-undated-section>
          <button
            type="button"
            class="flex items-center gap-2 px-4 pt-5 pb-1.5 text-left text-xs leading-5 font-medium text-tertiary tabular-nums"
            :aria-expanded="undatedOpen"
            @click="undatedOpen = !undatedOpen"
          >
            {{ $t('calendar-undated-button', { count: undated.jobs.length }) }}
            <Icon :name="undatedOpen ? 'chevronUp' : 'chevronDown'" size="xs" />
          </button>
          <DayList
            v-if="undatedOpen"
            :day="undated"
            :clock="clock"
            :locale="locale"
            empty=""
            @open="open"
            @patch="patch"
          />
        </section>
      </template>
    </AgendaList>

    <MonthGrid
      v-else-if="!narrow"
      :month="month"
      :weeks="weeks"
      :by-day="byDay"
      :overlays-by-day="overlaysByDay"
      :clock="clock"
      :locale="locale"
      :timed="timed"
      :focus-day="focusDay"
      :active-day="listDay"
      :drop-day="drag.dropDay.value"
      :dragged-id="drag.draggedId.value"
      :label="dayLabel"
      :aria-label="monthTitle(month, locale)"
      @open="open"
      @day="openDay"
      @move="moveTo"
      @item-down="onItemDown"
    />

    <!-- Narrow: the month, then the selected day, then the jobs with no
         date, one after the other in the page's flow. -->
    <div v-else class="flex min-h-0 flex-1 flex-col overflow-y-auto overscroll-contain pb-8" data-calendar-scroll data-calendar-rows>
      <MonthCompact
        :month="month"
        :weeks="weeks"
        :by-day="byDay"
        :clock="clock"
        :locale="locale"
        :focus-day="focusDay"
        :selected-day="focusDay"
        :label="dayLabel"
        :aria-label="monthTitle(month, locale)"
        @select="moveTo"
        @move="moveTo"
      />

      <section class="flex flex-col" :aria-label="dayTitle(focusDay, locale)" data-day-section>
        <div class="flex flex-wrap items-baseline gap-x-3 px-4 pt-4 pb-1.5">
          <h3 class="text-sm leading-5 font-semibold text-primary">{{ dayTitle(focusDay, locale) }}</h3>
          <p v-if="jobsOn(byDay, focusDay).jobs.length" class="text-xs leading-5 text-secondary tabular-nums">
            {{ load(jobsOn(byDay, focusDay).jobs) }}
          </p>
        </div>
        <DayList
          :day="jobsOn(byDay, focusDay)"
          :overlays="overlaysByDay.get(focusDay)"
          :clock="clock"
          :locale="locale"
          :empty="$t('plan-day-empty')"
          @open="open"
          @patch="patch"
        />
      </section>

      <section v-if="undated.jobs.length" class="flex flex-col" data-undated-section>
        <button
          type="button"
          class="flex items-center gap-2 px-4 pt-5 pb-1.5 text-left text-xs leading-5 font-medium text-tertiary tabular-nums"
          :aria-expanded="undatedOpen"
          @click="undatedOpen = !undatedOpen"
        >
          {{ $t('calendar-undated-button', { count: undated.jobs.length }) }}
          <Icon :name="undatedOpen ? 'chevronUp' : 'chevronDown'" size="xs" />
        </button>
        <DayList
          v-if="undatedOpen"
          :day="undated"
          :clock="clock"
          :locale="locale"
          empty=""
          @open="open"
          @patch="patch"
        />
      </section>
    </div>

    <!-- Wide: a day read whole. -->
    <Popover
      :open="range === 'month' && !narrow && listDay != null"
      :anchor="dayAnchor"
      placement="bottom-start"
      :offset="listOffset"
      role="dialog"
      :aria-label="listDay ? dayTitle(listDay, locale) : undefined"
      :popover-class="POPOVER"
      :auto-focus="focusRow == null"
      @close="listDay = null"
    >
      <div v-if="listDay" class="flex min-h-0 flex-col" data-day-popover data-calendar-rows>
        <div class="flex shrink-0 items-start gap-2 px-4 pt-3 pb-2">
          <div class="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-3">
            <h3 class="text-sm leading-7 font-semibold text-primary">{{ dayTitle(listDay, locale) }}</h3>
            <p v-if="listJobs.jobs.length" class="text-xs leading-5 text-secondary tabular-nums">{{ load(listJobs.jobs) }}</p>
          </div>
        </div>
        <DayList
          class="min-h-0 overflow-y-auto overscroll-contain"
          :day="listJobs"
          :overlays="listOverlays"
          :clock="clock"
          :locale="locale"
          :empty="$t('plan-day-empty')"
          @open="open"
          @patch="patch"
        />
      </div>
    </Popover>

    <!-- Wide: the jobs with no date. -->
    <Popover
      :open="!narrow && undatedOpen"
      :anchor="undatedAnchor"
      placement="bottom-end"
      role="dialog"
      :aria-label="$t('calendar-undated-title')"
      :popover-class="POPOVER"
      @close="undatedOpen = false"
    >
      <div class="flex min-h-0 flex-col" data-undated-popover data-calendar-rows>
        <div class="flex shrink-0 items-start gap-2 px-4 pt-3 pb-2">
          <h3 class="min-w-0 flex-1 text-sm leading-7 font-semibold text-primary">{{ $t('calendar-undated-title') }}</h3>
        </div>
        <DayList
          class="min-h-0 overflow-y-auto overscroll-contain"
          :day="undated"
          :clock="clock"
          :locale="locale"
          empty=""
          @open="open"
          @patch="patch"
        />
      </div>
    </Popover>

    <TicketDragPreview
      v-if="dragged"
      :ticket="{
        id: dragged.id,
        number: dragged.number,
        title: dragged.title,
        category: dragged.workflow_state.category,
        assigneeUuid: dragged.assignee_uuid,
        priority: dragged.priority,
      }"
      :position="drag.position"
    />
  </div>
</template>
