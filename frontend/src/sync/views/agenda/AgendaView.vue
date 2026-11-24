<script setup lang="ts">
/**
 * My day as the `agenda` view shape: one day, read as a sequence.
 *
 *   Overdue   today only: open jobs from an earlier day
 *   Booked    jobs with a time, in time order
 *   Any time  the rest: placed by hand first, then by priority
 *   No date   today only: open jobs with no day at all
 *
 * Overdue and No date keep in sight what My day listed before it had
 * an order; they are not part of the day's order, so their rows don't
 * drag. Moving them onto the day comes with "Move to today".
 *
 * One column, at most 720px wide, on the list area's left edge. The day
 * is the title; the load ("11 jobs, about 8.5 h") follows it. Dragging
 * writes a rank between the new neighbours; a drop into Booked gives the
 * job a time and opens the time field on it; a drop out of Booked clears
 * the time.
 *
 * The parent owns the day (so its selection follows the jobs on screen)
 * and splits it with `agendaFor`; everything derives from the pool, so the
 * view paints at once and has no loading state.
 */
import { computed, nextTick, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import ContextMenu from '@/components/common/ContextMenu.vue'
import IconButton from '@/components/common/IconButton.vue'
import TicketDragPreview from '@/components/common/TicketDragPreview.vue'
import type { MenuItem } from '@/components/common/MenuList.vue'
import TimePopover from '@/components/planning/TimePopover.vue'
import type { PopoverAnchor } from '@/composables/popoverAnchor'
import { daysBetweenDays, formatDay, type Day } from '@nosdesk/core/utils/ticketDates'
import {
  loadHours,
  loadMinutes,
  planTime,
  planTimePatch,
  SIZE_MINUTES,
  SIZES,
  sizeOf,
  type Size,
} from '@nosdesk/core/utils/ticketPlan'
import { overlaps, prefillTime, ranksForDrop, type AgendaPlan } from '@nosdesk/core/sync/views/planOrder'
import type { CardData } from '@nosdesk/core/sync/views/types'
import { useRowReorder, type RowDrop } from '../useRowReorder'
import AgendaRow from './AgendaRow.vue'

const props = defineProps<{
  /** The day's jobs, split by `agendaFor`. */
  plan: AgendaPlan
  today: Day
  /** The ticket the preview pane is showing, if any. */
  selectedId?: number | null
  /** The list's own row actions, shown under the planning ones. */
  extraMenuItems?: MenuItem[]
}>()

const day = defineModel<Day>('day', { required: true })

const emit = defineEmits<{
  (e: 'open', id: number): void
  (e: 'patch', id: number, patch: Partial<CardData>): void
  /** The row menu opened on a job, so the parent can fill its items. */
  (e: 'menu-open', id: number): void
  /** One of `extraMenuItems` was chosen. */
  (e: 'menu-action', action: string): void
}>()

const fluent = useFluent()

// --- The day -----------------------------------------------------------

function shiftDay(by: number): void {
  const date = new Date(`${day.value}T00:00:00Z`)
  date.setUTCDate(date.getUTCDate() + by)
  day.value = date.toISOString().slice(0, 10)
}

const title = computed(() => formatDay(day.value, 'weekdayLongMonthDay'))

const overlapping = computed(() => overlaps(props.plan.booked))
const count = computed(() => props.plan.booked.length + props.plan.anyTime.length)

/** "11 jobs, about 8.5 h": a number to judge the day by, nothing more. */
const load = computed(() => {
  const minutes = loadMinutes([...props.plan.booked, ...props.plan.anyTime])
  return minutes > 0
    ? fluent.$t('plan-day-load-sized', { count: count.value, hours: loadHours(minutes) })
    : fluent.$t('plan-day-load', { count: count.value })
})

/** The first job in Any time that has not been placed by hand, when
 *  placed jobs sit above it: a rule marks where the hand order ends. */
const firstUnplacedId = computed(() => {
  const list = props.plan.anyTime
  const index = list.findIndex((c) => c.rank == null)
  return index > 0 ? list[index].id : null
})

const isToday = computed(() => daysBetweenDays(props.today, day.value) === 0)

const byId = (id: number) =>
  props.plan.booked.find((c) => c.id === id) ??
  props.plan.anyTime.find((c) => c.id === id) ??
  props.plan.overdue.find((c) => c.id === id) ??
  props.plan.undated.find((c) => c.id === id)

/** Overdue and No date rows: outside the day's order. */
const isFixed = (id: number) =>
  props.plan.overdue.some((c) => c.id === id) || props.plan.undated.some((c) => c.id === id)

// --- Time and size -----------------------------------------------------

const root = ref<HTMLElement | null>(null)
const gutterOf = (id: number) => () =>
  root.value?.querySelector<HTMLElement>(`[data-row="${id}"] > div`) ?? null

const timeFor = ref<number | null>(null)
const timeCard = computed(() => (timeFor.value != null ? byId(timeFor.value) : undefined))
const timeAnchor = computed<PopoverAnchor>(() => ({
  type: 'element',
  element: timeFor.value != null ? gutterOf(timeFor.value) : () => null,
}))

function setTime(id: number, time: string | null): void {
  const card = byId(id)
  if (!card) return
  emit('patch', id, planTimePatch(card, time))
}

// --- Order -------------------------------------------------------------

function place(id: number, slot: number): void {
  const card = byId(id)
  if (!card) return
  const anyTime = props.plan.anyTime
  const order = anyTime.some((c) => c.id === id) ? anyTime : [...anyTime, card]
  for (const write of ranksForDrop(order, id, slot)) {
    const patch: Partial<CardData> = { rank: write.rank }
    // Out of Booked: the drop clears the time it had.
    if (write.id === id && planTime(card) != null) Object.assign(patch, planTimePatch(card, null))
    emit('patch', write.id, patch)
  }
}

function onDrop(drop: RowDrop): void {
  const card = byId(drop.id)
  if (!card) return
  if (drop.lane === 'any') {
    place(drop.id, drop.slot)
    return
  }
  // Into Booked: the job takes the time the slot suggests, and the time
  // field opens on it to confirm or change.
  const others = props.plan.booked.filter((c) => c.id !== drop.id)
  setTime(drop.id, `${prefillTime(others, drop.slot, card)}:00`)
  void nextTick(() => (timeFor.value = drop.id))
}

const reorder = useRowReorder({
  root: () => root.value,
  onDrop,
  onClick: (id) => emit('open', id),
})

function moveBy(id: number, by: -1 | 1): void {
  const index = props.plan.anyTime.findIndex((c) => c.id === id)
  if (index < 0) return
  const slot = index + by
  if (slot < 0 || slot >= props.plan.anyTime.length) return
  place(id, slot)
  void nextTick(() => root.value?.querySelector<HTMLElement>(`[data-row="${id}"]`)?.focus())
}

const dragged = computed(() => (reorder.draggedId.value != null ? byId(reorder.draggedId.value) : undefined))

// --- Row menu ----------------------------------------------------------

const menu = ref<{ id: number; x: number; y: number } | null>(null)
const menuOpen = ref(false)

function openMenu(id: number, event: MouseEvent): void {
  // A long press on a phone lands here too; there it picks the row up.
  if (reorder.settling.value || reorder.lastPointerType() !== 'mouse') return
  menu.value = { id, x: event.clientX, y: event.clientY }
  menuOpen.value = true
  emit('menu-open', id)
}

const PLAN_ACTIONS = new Set(['time', 'clear-time', 'top', 'size-none', ...SIZES.map((s) => `size-${s}`)])

const menuItems = computed<MenuItem[]>(() => {
  const card = menu.value ? byId(menu.value.id) : undefined
  if (!card) return []
  const timed = planTime(card) != null
  const size = sizeOf(card.estimate_minutes)
  const undated = props.plan.undated.some((c) => c.id === card.id)
  const items: MenuItem[] = []
  // A time needs a day, and only the day's own jobs have a place in it.
  if (!undated) {
    items.push({ id: 'time', label: fluent.$t(timed ? 'plan-menu-change-time' : 'plan-menu-set-time'), icon: 'clock' })
  }
  if (timed) items.push({ id: 'clear-time', label: fluent.$t('plan-menu-clear-time') })
  else if (!isFixed(card.id) && props.plan.anyTime[0]?.id !== card.id) {
    items.push({ id: 'top', label: fluent.$t('plan-menu-move-top'), icon: 'arrowUp' })
  }
  items.push({ id: 'size', label: fluent.$t('plan-size-label'), heading: true, divider: items.length > 0 })
  for (const s of SIZES) {
    items.push({
      id: `size-${s}`,
      label: fluent.$t(`plan-size-${s}`),
      trailing: fluent.$t(`plan-size-length-${s}`),
      checked: size === s,
    })
  }
  items.push({ id: 'size-none', label: fluent.$t('plan-size-none'), checked: size === null })
  const extra = props.extraMenuItems ?? []
  if (extra.length > 0) items.push({ ...extra[0], divider: true }, ...extra.slice(1))
  return items
})

function onMenuSelect(action: string): void {
  const id = menu.value?.id
  menuOpen.value = false
  if (id == null) return
  if (!PLAN_ACTIONS.has(action)) emit('menu-action', action)
  else if (action === 'time') void nextTick(() => (timeFor.value = id))
  else if (action === 'clear-time') setTime(id, null)
  else if (action === 'top') place(id, 0)
  else {
    const s = action.slice(5)
    emit('patch', id, { estimate_minutes: s === 'none' ? null : SIZE_MINUTES[s as Size] })
  }
}
</script>

<template>
  <div ref="root" class="min-h-0 overflow-y-auto overscroll-contain bg-surface" data-agenda>
    <div class="flex w-full max-w-[45rem] flex-col pb-8">
      <!-- The day, its load, and the way to another day. -->
      <header class="flex items-start gap-2 px-4 pt-4 pb-1">
        <div class="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-3">
          <h2 class="text-lg leading-7 font-semibold text-primary">{{ title }}</h2>
          <p v-if="count > 0" class="text-xs leading-5 text-secondary tabular-nums" data-load>{{ load }}</p>
        </div>
        <div class="flex h-7 shrink-0 items-center gap-0.5">
          <button
            v-if="daysBetweenDays(today, day) !== 0"
            type="button"
            class="h-7 rounded-md px-2 text-xs text-secondary transition-colors hover:bg-surface-hover hover:text-primary"
            @click="day = today"
          >{{ $t('plan-day-today') }}</button>
          <IconButton size="sm" icon="chevronLeft" :label="$t('plan-day-previous')" @click="shiftDay(-1)" />
          <IconButton size="sm" icon="chevronRight" :label="$t('plan-day-next')" @click="shiftDay(1)" />
        </div>
      </header>

      <section v-if="plan.overdue.length > 0" data-section="overdue" :aria-label="$t('tickets-due-overdue')">
        <h3 class="px-4 pt-3 pb-1.5 text-xs font-medium text-tertiary">{{ $t('tickets-due-overdue') }}</h3>
        <ul class="flex flex-col border-t border-subtle/40">
          <AgendaRow
            v-for="card in plan.overdue"
            :key="card.id"
            :card="card"
            :today="today"
            :selected="selectedId === card.id"
            :time-open="timeFor === card.id"
            fixed
            @open="emit('open', card.id)"
            @open-time="timeFor = timeFor === card.id ? null : card.id"
            @size="(minutes) => emit('patch', card.id, { estimate_minutes: minutes })"
            @menu="(e) => openMenu(card.id, e)"
          />
        </ul>
      </section>

      <p v-if="count === 0" class="px-4 py-3 text-sm text-tertiary">
        {{ isToday ? $t('ticket-list-my-day-nothing-today') : $t('plan-day-empty') }}
      </p>

      <template v-else>
        <section data-lane="booked" :aria-label="$t('plan-section-booked')">
          <h3 class="px-4 pt-3 pb-1.5 text-xs font-medium text-tertiary">{{ $t('plan-section-booked') }}</h3>
          <ul class="flex flex-col border-t border-subtle/40">
            <AgendaRow
              v-for="card in plan.booked"
              :key="card.id"
              :card="card"
              :today="today"
              :overlap="overlapping.get(card.id)"
              :selected="selectedId === card.id"
              :lifted="reorder.draggedId.value === card.id"
              :time-open="timeFor === card.id"
              @row-down="(e) => reorder.onRowDown(card.id, e)"
              @open="emit('open', card.id)"
              @open-time="timeFor = timeFor === card.id ? null : card.id"
              @size="(minutes) => emit('patch', card.id, { estimate_minutes: minutes })"
              @menu="(e) => openMenu(card.id, e)"
            />
          </ul>
        </section>

        <section data-lane="any" :aria-label="$t('plan-section-any-time')">
          <h3 class="px-4 pt-4 pb-1.5 text-xs font-medium text-tertiary">{{ $t('plan-section-any-time') }}</h3>
          <ul class="flex flex-col border-t border-subtle/40">
            <template v-for="card in plan.anyTime" :key="card.id">
              <li
                v-if="card.id === firstUnplacedId"
                role="separator"
                class="-mt-px border-t border-strong"
                data-placed-rule
              />
              <AgendaRow
                :card="card"
                :today="today"
                :selected="selectedId === card.id"
                :lifted="reorder.draggedId.value === card.id"
                @row-down="(e) => reorder.onRowDown(card.id, e)"
                @grip-down="(e) => reorder.onGripDown(card.id, e)"
                @open="emit('open', card.id)"
                @size="(minutes) => emit('patch', card.id, { estimate_minutes: minutes })"
                @menu="(e) => openMenu(card.id, e)"
                @move="(by) => moveBy(card.id, by)"
              />
            </template>
          </ul>
        </section>
      </template>

      <section v-if="plan.undated.length > 0" data-section="undated" :aria-label="$t('tickets-due-none')">
        <h3 class="px-4 pt-4 pb-1.5 text-xs font-medium text-tertiary">{{ $t('tickets-due-none') }}</h3>
        <ul class="flex flex-col border-t border-subtle/40">
          <AgendaRow
            v-for="card in plan.undated"
            :key="card.id"
            :card="card"
            :today="today"
            :selected="selectedId === card.id"
            fixed
            @open="emit('open', card.id)"
            @size="(minutes) => emit('patch', card.id, { estimate_minutes: minutes })"
            @menu="(e) => openMenu(card.id, e)"
          />
        </ul>
      </section>
    </div>

    <!-- Where the drop lands. Rows stay put until it does. -->
    <Teleport to="body">
      <div
        v-if="reorder.line.value"
        class="pointer-events-none fixed z-overlay h-0.5 -translate-y-px rounded-full bg-accent"
        data-drop-line
        :style="{
          top: `${reorder.line.value.top}px`,
          left: `${reorder.line.value.left}px`,
          width: `${reorder.line.value.width}px`,
        }"
      />
    </Teleport>
    <TicketDragPreview
      v-if="dragged && reorder.state.dragPosition"
      :ticket="{
        id: dragged.id,
        number: dragged.number,
        title: dragged.title,
        category: dragged.workflow_state.category,
        assigneeUuid: dragged.assignee_uuid,
        priority: dragged.priority,
      }"
      :position="reorder.state.dragPosition"
    />

    <TimePopover
      :open="timeCard != null"
      :anchor="timeAnchor"
      :time="timeCard ? planTime(timeCard) : null"
      :title="$t('plan-time-title')"
      @update="(time) => timeFor != null && setTime(timeFor, time)"
      @close="timeFor = null"
    />
    <ContextMenu
      :items="menuItems"
      :x="menu?.x ?? 0"
      :y="menu?.y ?? 0"
      :open="menuOpen"
      :label="menu ? $t('plan-menu-aria', { number: String(byId(menu.id)?.number ?? '') }) : undefined"
      @select="onMenuSelect"
      @close="menuOpen = false"
    />
  </div>
</template>
