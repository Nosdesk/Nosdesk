<script setup lang="ts">
/**
 * SLA admin: list + edit working calendars and SLA policies.
 *
 * Layout mirrors the dominant admin idiom in this codebase
 * (WebhooksView, ApiTokensView, AssignmentRulesView): a single
 * column centred in a max-w-8xl container, with each list living
 * inside its own SectionCard and a "+ New" button in the card
 * header that opens a modal. Calendars come first because policies
 * depend on them — the reading order encodes the dependency.
 *
 * Both calendars and policies use a unified modal that handles
 * create and edit through the same form; mode is determined by
 * whether the corresponding `*Editing` ref is null. This matches
 * what AssignmentRules + Webhooks already do and saves a second
 * form template.
 *
 * Reads go through Pinia Colada so revisits paint from cache and
 * background revalidations stay silent; mutations push the new row
 * into the cache directly rather than refetching. A first-load
 * skeleton mirrors the live shape so the cutover doesn't reflow.
 *
 * The policy form is grouped into Conditions (which tickets the
 * policy matches) and Targets (the times it sets) so the admin
 * reads top to bottom in the same order the engine evaluates:
 * filter, then compute.
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery, useQueryCache } from '@pinia/colada'
import {
  slaService,
  type SlaPolicy,
  type WorkingCalendar,
  type WorkingCalendarBody,
  type WorkingCalendarHoliday,
  type SlaPolicyBody,
  type PolicyMatchCounts,
} from '@/services/slaService'
import { categoryService } from '@/services/categoryService'
import type { TicketCategory } from '@/types/category'
import { groupService } from '@/services/groupService'
import type { GroupWithMemberCount } from '@/types/group'
import Checkbox from '@/components/common/Checkbox.vue'
import Button from '@/components/common/Button.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import FormInput from '@/components/common/FormInput.vue'
import FormNumber from '@/components/common/FormNumber.vue'
import Skeleton from '@/components/common/Skeleton.vue'
import SkeletonBar from '@/components/common/SkeletonBar.vue'
import Modal from '@/components/Modal.vue'
import Icon from '@/components/common/Icon.vue'
import SearchableDropdown from '@/components/common/SearchableDropdown.vue'
import { useTimezoneOptions } from '@/composables/useTimezoneOptions'
import { GROUPS_QUERY_KEY } from '@/composables/useAssignmentPickerQueries'
import WeekScheduleEditor, {
  type WeekSchedule,
} from '@/components/admin/WeekScheduleEditor.vue'
import DatePicker from '@/components/common/DatePicker.vue'
import {
  HOLIDAY_TEMPLATE_LIST,
  HOLIDAY_TEMPLATES,
  type CountryCode,
} from '@/data/holidayTemplates'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

// ---------------- Query keys + queries ----------------
const POLICIES_KEY = ['sla', 'policies'] as const
const CALENDARS_KEY = ['sla', 'calendars'] as const
const CATEGORIES_KEY = ['categories'] as const
const GROUPS_KEY = GROUPS_QUERY_KEY
const MATCH_COUNTS_KEY = ['sla', 'policy-matches'] as const

const MATCH_COUNTS_REFRESH_MS = 30_000

const queryCache = useQueryCache()
const policiesQuery = useQuery({
  key: POLICIES_KEY,
  query: () => slaService.listPolicies(),
})
const calendarsQuery = useQuery({
  key: CALENDARS_KEY,
  query: () => slaService.listCalendars(),
})
const categoriesQuery = useQuery({
  key: CATEGORIES_KEY,
  query: () => categoryService.getCategories(),
})
const groupsQuery = useQuery({
  key: GROUPS_KEY,
  query: () => groupService.getGroups(),
})
const matchCountsQuery = useQuery({
  key: MATCH_COUNTS_KEY,
  query: () => slaService.getPolicyMatchCounts(),
})

// Refresh the live match counts on a steady tick so the breach /
// at-risk callouts reflect the current open-ticket set without the
// admin having to reload. 30s mirrors the existing SLA pill tick and
// the breach-detection job cadence on the backend.
let matchCountsTimer: ReturnType<typeof setInterval> | undefined
onMounted(() => {
  matchCountsTimer = setInterval(() => {
    matchCountsQuery.refetch()
  }, MATCH_COUNTS_REFRESH_MS)
})
onBeforeUnmount(() => {
  if (matchCountsTimer) clearInterval(matchCountsTimer)
})

const policies = computed<SlaPolicy[]>(() => policiesQuery.data.value ?? [])
const calendars = computed<WorkingCalendar[]>(() => calendarsQuery.data.value ?? [])
const categories = computed<TicketCategory[]>(() => categoriesQuery.data.value ?? [])
const groups = computed<GroupWithMemberCount[]>(() => groupsQuery.data.value ?? [])
const matchCounts = computed<Record<string, PolicyMatchCounts>>(
  () => matchCountsQuery.data.value ?? {},
)

function countsFor(policyId: number): PolicyMatchCounts {
  return (
    matchCounts.value[String(policyId)] ?? {
      total: 0,
      on_track: 0,
      at_risk: 0,
      breached: 0,
      paused: 0,
    }
  )
}

const isFirstLoad = computed(
  () =>
    (policiesQuery.status.value === 'pending' &&
      policiesQuery.data.value === undefined) ||
    (calendarsQuery.status.value === 'pending' &&
      calendarsQuery.data.value === undefined),
)

const loadError = computed(() => {
  if (
    policiesQuery.error.value ||
    calendarsQuery.error.value ||
    categoriesQuery.error.value ||
    groupsQuery.error.value
  ) {
    return t('admin-sla-error-load')
  }
  return ''
})

const error = ref<string | null>(null)
const liveError = computed(() => error.value ?? loadError.value ?? null)

// ---------------- Confirm delete ----------------
const pendingDelete = ref<
  | { kind: 'calendar'; id: number; name: string }
  | { kind: 'policy'; id: number; name: string }
  | null
>(null)
const confirmDeleteMessage = computed<string>(() => {
  if (!pendingDelete.value) return ''
  return pendingDelete.value.kind === 'calendar'
    ? t('admin-sla-calendar-delete-confirm')
    : t('admin-sla-policy-delete-confirm')
})

// ---------------- Calendar modal (create + edit) ----------------
// One draft + one "editing target" ref drives both modes. When the
// target is null we're creating; otherwise the draft is seeded from
// the target and Save patches the existing row.
const calendarOpen = ref(false)
const editingCalendar = ref<WorkingCalendar | null>(null)
const calendarDraft = ref<WorkingCalendarBody>(emptyCalendarDraft())

function emptyCalendarDraft(): WorkingCalendarBody {
  return {
    name: '',
    timezone: 'UTC',
    schedule: {
      mon: [['09:00', '17:00']],
      tue: [['09:00', '17:00']],
      wed: [['09:00', '17:00']],
      thu: [['09:00', '17:00']],
      fri: [['09:00', '17:00']],
      sat: [],
      sun: [],
    },
    is_default: false,
  }
}

const calendarModalTitle = computed(() =>
  editingCalendar.value
    ? t('admin-sla-edit-calendar-title')
    : t('admin-sla-new-calendar-title'),
)

function openCreateCalendar(): void {
  editingCalendar.value = null
  calendarDraft.value = emptyCalendarDraft()
  editingHolidays.value = []
  newHolidayDate.value = ''
  newHolidayLabel.value = ''
  newHolidayAnnual.value = false
  calendarOpen.value = true
}

async function openEditCalendar(cal: WorkingCalendar): Promise<void> {
  editingCalendar.value = cal
  // Deep-clone the schedule so edits don't leak into the cached row
  // before the user clicks Save.
  calendarDraft.value = {
    name: cal.name,
    timezone: cal.timezone,
    // Deep-clone so edits don't leak into the cached row before the
    // user clicks Save. structuredClone is the modern equivalent of
    // the JSON-stringify-parse idiom and survives Date / Map / etc.
    schedule: structuredClone(cal.schedule),
    is_default: cal.is_default,
  }
  editingHolidays.value = []
  newHolidayDate.value = ''
  newHolidayLabel.value = ''
  newHolidayAnnual.value = false
  calendarOpen.value = true
  try {
    editingHolidays.value = await slaService.listHolidays(cal.id)
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('admin-sla-error-load')
  }
}

function closeCalendarModal(): void {
  calendarOpen.value = false
  editingCalendar.value = null
}

async function saveCalendar(): Promise<void> {
  if (!calendarDraft.value.name.trim()) return
  // Clear any stale error from a prior failed attempt so the banner
  // disappears the moment the user retries, not just on success.
  error.value = null
  try {
    if (editingCalendar.value) {
      const target = editingCalendar.value
      const updated = await slaService.updateCalendar(target.id, calendarDraft.value)
      queryCache.setQueryData(
        CALENDARS_KEY,
        calendars.value.map((c) => (c.id === target.id ? updated : c)),
      )
    } else {
      const created = await slaService.createCalendar(calendarDraft.value)
      queryCache.setQueryData(CALENDARS_KEY, [...calendars.value, created])
    }
    closeCalendarModal()
    error.value = null
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('admin-sla-error-save')
  }
}

function requestDeleteCalendar(cal: WorkingCalendar): void {
  pendingDelete.value = { kind: 'calendar', id: cal.id, name: cal.name }
}

async function deleteCalendar(id: number): Promise<void> {
  try {
    await slaService.deleteCalendar(id)
    queryCache.setQueryData(
      CALENDARS_KEY,
      calendars.value.filter((c) => c.id !== id),
    )
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('admin-sla-error-delete')
  }
}

async function toggleCalendarDefault(cal: WorkingCalendar): Promise<void> {
  try {
    const updated = await slaService.updateCalendar(cal.id, {
      name: cal.name,
      timezone: cal.timezone,
      schedule: cal.schedule,
      is_default: !cal.is_default,
    })
    queryCache.setQueryData(
      CALENDARS_KEY,
      calendars.value.map((c) => (c.id === cal.id ? updated : c)),
    )
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('admin-sla-error-update')
  }
}

// ---------------- Holidays (scoped to the calendar being edited) ----------------
// Loaded lazily into a local ref because the admin only looks at a
// calendar's holidays while its modal is open; no app-wide cache
// scope justified.
const editingHolidays = ref<WorkingCalendarHoliday[]>([])
const newHolidayDate = ref('')
const newHolidayLabel = ref('')
const newHolidayAnnual = ref(false)
// Set false the moment the schedule editor reports an invalid range
// (close <= open). The Save button binds to this so a typo can't
// quietly save and get dropped by the backend's schedule parser.
const calendarScheduleValid = ref(true)

async function addHoliday(): Promise<void> {
  const target = editingCalendar.value
  if (!target || !newHolidayDate.value) return
  try {
    const created = await slaService.createHoliday(target.id, {
      date: newHolidayDate.value,
      label: newHolidayLabel.value.trim() || null,
      recurrence: newHolidayAnnual.value ? 'annual' : 'none',
    })
    editingHolidays.value = [...editingHolidays.value, created].sort((a, b) =>
      a.date.localeCompare(b.date),
    )
    newHolidayDate.value = ''
    newHolidayLabel.value = ''
    newHolidayAnnual.value = false
    error.value = null
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('admin-sla-error-create')
  }
}

// Bulk holiday import. The picker holds the country choice; on
// change we POST each preset, swallow per-row unique-violation 400s
// (the backend rejects duplicates on (calendar_id, date)) and surface
// a transient summary so the admin sees what landed. The picker
// resets after the run so re-selecting the same country re-imports.
const importChoice = ref<CountryCode | ''>('')
const importSummary = ref<string | null>(null)
let importSummaryTimer: ReturnType<typeof setTimeout> | undefined

function flashImportSummary(message: string): void {
  importSummary.value = message
  if (importSummaryTimer) clearTimeout(importSummaryTimer)
  importSummaryTimer = setTimeout(() => {
    importSummary.value = null
  }, 4500)
}

async function handleImportChoice(country: CountryCode | ''): Promise<void> {
  if (!country) return
  const target = editingCalendar.value
  if (!target) return
  const template = HOLIDAY_TEMPLATES[country]
  const presets = template.generate(new Date().getFullYear())
  let added = 0
  let skipped = 0
  for (const p of presets) {
    try {
      const created = await slaService.createHoliday(target.id, {
        date: p.date,
        label: p.label,
        recurrence: p.recurrence,
      })
      editingHolidays.value = [...editingHolidays.value, created]
      added++
    } catch {
      // Duplicate (HTTP 400 from the unique constraint) or transient
      // network error — count and keep going. The admin sees the
      // summary at the end either way.
      skipped++
    }
  }
  editingHolidays.value = editingHolidays.value
    .slice()
    .sort((a, b) => a.date.localeCompare(b.date))
  importChoice.value = ''
  flashImportSummary(
    t('admin-sla-holiday-import-summary', {
      country: template.name,
      added,
      skipped,
    }),
  )
}

async function removeHoliday(id: number): Promise<void> {
  try {
    await slaService.deleteHoliday(id)
    editingHolidays.value = editingHolidays.value.filter((h) => h.id !== id)
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('admin-sla-error-delete')
  }
}

// ---------------- Policy modal (create + edit) ----------------
const policyOpen = ref(false)
const editingPolicy = ref<SlaPolicy | null>(null)
const policyDraft = ref<SlaPolicyBody>(emptyPolicyDraft())

function emptyPolicyDraft(): SlaPolicyBody {
  return {
    name: '',
    target_response_minutes: 60,
    target_resolution_minutes: 24 * 60,
    working_calendar_id: null,
    priority_filter: null,
    category_id_filter: null,
    assignee_group_id_filter: null,
    is_default: false,
  }
}

const policyModalTitle = computed(() =>
  editingPolicy.value
    ? t('admin-sla-edit-policy-title')
    : t('admin-sla-new-policy-title'),
)

function openCreatePolicy(): void {
  editingPolicy.value = null
  policyDraft.value = emptyPolicyDraft()
  policyOpen.value = true
}

function openEditPolicy(p: SlaPolicy): void {
  editingPolicy.value = p
  policyDraft.value = {
    name: p.name,
    target_response_minutes: p.target_response_minutes,
    target_resolution_minutes: p.target_resolution_minutes,
    working_calendar_id: p.working_calendar_id,
    priority_filter: p.priority_filter,
    category_id_filter: p.category_id_filter,
    assignee_group_id_filter: p.assignee_group_id_filter,
    is_default: p.is_default,
  }
  policyOpen.value = true
}

function closePolicyModal(): void {
  policyOpen.value = false
  editingPolicy.value = null
}

async function savePolicy(): Promise<void> {
  if (!policyDraft.value.name.trim()) return
  error.value = null
  try {
    if (editingPolicy.value) {
      const target = editingPolicy.value
      const updated = await slaService.updatePolicy(target.id, policyDraft.value)
      queryCache.setQueryData(
        POLICIES_KEY,
        policies.value.map((p) => (p.id === target.id ? updated : p)),
      )
    } else {
      const created = await slaService.createPolicy(policyDraft.value)
      queryCache.setQueryData(POLICIES_KEY, [...policies.value, created])
    }
    closePolicyModal()
    error.value = null
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('admin-sla-error-save')
  }
}

function requestDeletePolicy(p: SlaPolicy): void {
  // Confirm wording for policy delete calls out the side effect: any
  // tickets the policy currently covers stop having an SLA.
  pendingDelete.value = { kind: 'policy', id: p.id, name: p.name }
}

async function deletePolicy(id: number): Promise<void> {
  try {
    await slaService.deletePolicy(id)
    queryCache.setQueryData(
      POLICIES_KEY,
      policies.value.filter((p) => p.id !== id),
    )
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('admin-sla-error-delete')
  }
}

async function confirmDelete(): Promise<void> {
  const target = pendingDelete.value
  if (!target) return
  pendingDelete.value = null
  if (target.kind === 'calendar') await deleteCalendar(target.id)
  else await deletePolicy(target.id)
}

async function togglePolicyDefault(p: SlaPolicy): Promise<void> {
  try {
    const updated = await slaService.updatePolicy(p.id, {
      name: p.name,
      target_response_minutes: p.target_response_minutes,
      target_resolution_minutes: p.target_resolution_minutes,
      working_calendar_id: p.working_calendar_id,
      priority_filter: p.priority_filter,
      category_id_filter: p.category_id_filter,
      assignee_group_id_filter: p.assignee_group_id_filter,
      is_default: !p.is_default,
    })
    queryCache.setQueryData(
      POLICIES_KEY,
      policies.value.map((x) => (x.id === p.id ? updated : x)),
    )
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('admin-sla-error-update')
  }
}

// ---------------- Dropdown options ----------------
const calendarOptions = computed(() =>
  calendars.value.map((c) => ({ value: c.id, label: c.name })),
)
const categoryOptions = computed(() =>
  categories.value.map((c) => ({ value: c.id, label: c.name })),
)
const groupOptions = computed(() =>
  groups.value.map((g) => ({ value: g.id, label: g.name })),
)
const timezoneOptions = useTimezoneOptions()

// SearchableDropdown's modelValue is a required string but
// WorkingCalendarBody.timezone is `string | undefined`. The adapter
// coerces undefined to '' and back; '' saves as undefined which
// the backend defaults to UTC.
const calendarDraftTimezone = computed<string>({
  get: () => calendarDraft.value.timezone ?? '',
  set: (v) => (calendarDraft.value.timezone = v),
})

// ---------------- Display helpers ----------------
function calendarName(id: number | null): string {
  if (id == null) return '-'
  return calendars.value.find((c) => c.id === id)?.name ?? `#${id}`
}

function fmtMinutes(m: number | null): string {
  if (m == null) return '-'
  if (m < 60) return `${m}m`
  if (m < 24 * 60) return `${(m / 60).toFixed(m % 60 === 0 ? 0 : 1)}h`
  return `${(m / (24 * 60)).toFixed(m % (24 * 60) === 0 ? 0 : 1)}d`
}

// Mirrors FormInput's size="sm" field styling so the bare <select>
// and the bare number-input read as the same control. If a
// FormSelect / FormNumberInput primitive lands later this constant
// disappears.
const FIELD_CLASS_SM =
  'w-full bg-surface-alt border border-subtle rounded-lg text-primary px-3 py-1.5 text-sm ' +
  'placeholder-tertiary transition-colors ' +
  'focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:border-accent ' +
  'disabled:opacity-50 disabled:cursor-not-allowed'

const FIELD_LABEL_CLASS = 'text-xs font-medium text-tertiary uppercase tracking-wide'
</script>

<template>
  <div class="flex-1 overflow-y-auto">
    <div class="max-w-8xl mx-auto px-4 sm:px-6 py-6 flex flex-col gap-4">
      <!-- Standard admin header: title + description, no sticky chrome.
           Matches WebhooksView / ApiTokensView / AssignmentRulesView. -->
      <header>
        <h1 class="text-xl lg:text-2xl font-bold text-primary">{{ $t('admin-sla-title') }}</h1>
        <p class="text-sm text-secondary mt-1 max-w-2xl">
          {{ $t('admin-sla-description') }}
        </p>
      </header>

      <Skeleton
        v-if="isFirstLoad"
        :label="$t('admin-sla-loading')"
        class="flex flex-col gap-4"
      >
        <!-- Mirror the live stacked-sections shape so the cutover
             doesn't reflow when data arrives. -->
        <div
          v-for="section in 2"
          :key="section"
          class="border border-default rounded-xl overflow-hidden"
        >
          <SkeletonBar class="h-9 w-full" />
          <div
            v-for="row in 4"
            :key="row"
            class="flex items-center gap-3 px-3 h-9 border-t border-subtle"
          >
            <SkeletonBar class="h-2.5 flex-1" />
            <SkeletonBar class="h-2.5 w-16" />
            <SkeletonBar class="h-2.5 w-12" />
          </div>
        </div>
      </Skeleton>

      <template v-else>
        <p v-if="liveError" class="text-sm text-status-error">{{ liveError }}</p>

        <!-- Working calendars. First because policies depend on them;
             the reading order encodes the dependency the old
             side-by-side grid used spatial juxtaposition to convey. -->
        <SectionCard content-padding="">
          <template #title>{{ $t('admin-sla-calendars-heading') }}</template>
          <template #headerActions>
            <button
              type="button"
              class="inline-flex items-center gap-1 text-[11px] font-medium text-accent hover:underline"
              @click="openCreateCalendar"
            >
              <Icon name="add" class="w-3 h-3" />
              {{ $t('admin-sla-new-calendar-button') }}
            </button>
          </template>
          <div class="overflow-x-auto">
            <table class="w-full text-xs">
              <thead class="bg-surface-alt text-tertiary">
                <tr>
                  <th class="text-left px-3 py-2 font-medium">{{ $t('admin-sla-col-name') }}</th>
                  <th class="text-left px-3 py-2 font-medium">{{ $t('admin-sla-col-tz') }}</th>
                  <th class="text-left px-3 py-2 font-medium">{{ $t('admin-sla-col-default') }}</th>
                  <th class="px-3 py-2"></th>
                </tr>
              </thead>
              <tbody class="divide-y divide-subtle">
                <tr v-if="!calendars.length" class="bg-surface">
                  <td colspan="4" class="px-3 py-4 text-tertiary text-center">
                    {{ $t('admin-sla-no-calendars-hint') }}
                  </td>
                </tr>
                <tr v-for="cal in calendars" :key="cal.id" class="bg-surface">
                  <td class="px-3 py-2 text-primary">{{ cal.name }}</td>
                  <td class="px-3 py-2 text-secondary">{{ cal.timezone }}</td>
                  <td class="px-3 py-2">
                    <button
                      v-if="cal.is_default"
                      type="button"
                      class="text-[10px] uppercase tracking-wide font-semibold text-accent border border-accent/40 bg-accent/10 rounded px-1.5 py-0.5 hover:bg-accent/15 transition-colors"
                      @click="toggleCalendarDefault(cal)"
                    >
                      {{ $t('admin-sla-default-badge') }}
                    </button>
                    <button
                      v-else
                      type="button"
                      class="text-xs text-secondary hover:text-accent transition-colors"
                      @click="toggleCalendarDefault(cal)"
                    >
                      {{ $t('admin-sla-set-default') }}
                    </button>
                  </td>
                  <td class="px-3 py-2">
                    <div class="flex items-center justify-end gap-1">
                      <button
                        type="button"
                        class="p-1.5 text-secondary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
                        :aria-label="$t('admin-sla-edit')"
                        @click="openEditCalendar(cal)"
                      >
                        <Icon name="rename" class="w-3.5 h-3.5" />
                      </button>
                      <button
                        type="button"
                        class="p-1.5 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-md transition-colors"
                        :aria-label="$t('admin-sla-delete')"
                        @click="requestDeleteCalendar(cal)"
                      >
                        <Icon name="trash" class="w-3.5 h-3.5" />
                      </button>
                    </div>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </SectionCard>

        <!-- Policies. Sit below calendars because they pick one as
             their working-hours source. -->
        <SectionCard content-padding="">
          <template #title>{{ $t('admin-sla-policies-heading') }}</template>
          <template #headerActions>
            <button
              type="button"
              class="inline-flex items-center gap-1 text-[11px] font-medium text-accent hover:underline"
              @click="openCreatePolicy"
            >
              <Icon name="add" class="w-3 h-3" />
              {{ $t('admin-sla-new-policy-button') }}
            </button>
          </template>
          <div class="overflow-x-auto">
            <table class="w-full text-xs">
              <thead class="bg-surface-alt text-tertiary">
                <tr>
                  <th class="text-left px-3 py-2 font-medium">{{ $t('admin-sla-col-name') }}</th>
                  <th class="text-left px-3 py-2 font-medium">
                    {{ $t('admin-sla-col-targets') }}
                  </th>
                  <th class="text-left px-3 py-2 font-medium">
                    {{ $t('admin-sla-col-calendar') }}
                  </th>
                  <th class="text-left px-3 py-2 font-medium">
                    {{ $t('admin-sla-col-matches') }}
                  </th>
                  <th class="text-left px-3 py-2 font-medium">{{ $t('admin-sla-col-default') }}</th>
                  <th class="px-3 py-2"></th>
                </tr>
              </thead>
              <tbody class="divide-y divide-subtle">
                <tr v-if="!policies.length" class="bg-surface">
                  <td colspan="6" class="px-3 py-4 text-tertiary text-center">
                    {{ $t('admin-sla-no-policies-hint') }}
                  </td>
                </tr>
                <tr v-for="p in policies" :key="p.id" class="bg-surface">
                  <td class="px-3 py-2 text-primary">{{ p.name }}</td>
                  <td class="px-3 py-2 text-secondary tabular-nums whitespace-nowrap">
                    {{ fmtMinutes(p.target_response_minutes) }}
                    <span class="text-tertiary"> / </span>
                    {{ fmtMinutes(p.target_resolution_minutes) }}
                  </td>
                  <td class="px-3 py-2 text-secondary">
                    {{ calendarName(p.working_calendar_id) }}
                  </td>
                  <td class="px-3 py-2 whitespace-nowrap">
                    <template v-if="countsFor(p.id).total === 0">
                      <span class="text-tertiary">{{ $t('admin-sla-matches-none') }}</span>
                    </template>
                    <template v-else>
                      <span class="text-primary tabular-nums">{{
                        $t('admin-sla-matches-total', { count: countsFor(p.id).total })
                      }}</span>
                      <span
                        v-if="countsFor(p.id).at_risk > 0"
                        class="ml-2 text-status-warning tabular-nums"
                        :title="$t('admin-sla-matches-at-risk-title')"
                      >
                        ·
                        {{
                          $t('admin-sla-matches-at-risk', {
                            count: countsFor(p.id).at_risk,
                          })
                        }}
                      </span>
                      <span
                        v-if="countsFor(p.id).breached > 0"
                        class="ml-2 text-status-error font-medium tabular-nums"
                        :title="$t('admin-sla-matches-breached-title')"
                      >
                        ·
                        {{
                          $t('admin-sla-matches-breached', {
                            count: countsFor(p.id).breached,
                          })
                        }}
                      </span>
                    </template>
                  </td>
                  <td class="px-3 py-2">
                    <button
                      v-if="p.is_default"
                      type="button"
                      class="text-[10px] uppercase tracking-wide font-semibold text-accent border border-accent/40 bg-accent/10 rounded px-1.5 py-0.5 hover:bg-accent/15 transition-colors"
                      @click="togglePolicyDefault(p)"
                    >
                      {{ $t('admin-sla-default-badge') }}
                    </button>
                    <button
                      v-else
                      type="button"
                      class="text-xs text-secondary hover:text-accent transition-colors"
                      @click="togglePolicyDefault(p)"
                    >
                      {{ $t('admin-sla-set-default') }}
                    </button>
                  </td>
                  <td class="px-3 py-2">
                    <div class="flex items-center justify-end gap-1">
                      <button
                        type="button"
                        class="p-1.5 text-secondary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
                        :aria-label="$t('admin-sla-edit')"
                        @click="openEditPolicy(p)"
                      >
                        <Icon name="rename" class="w-3.5 h-3.5" />
                      </button>
                      <button
                        type="button"
                        class="p-1.5 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-md transition-colors"
                        :aria-label="$t('admin-sla-delete')"
                        @click="requestDeletePolicy(p)"
                      >
                        <Icon name="trash" class="w-3.5 h-3.5" />
                      </button>
                    </div>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </SectionCard>
      </template>
    </div>

    <ConfirmModal
      :show="pendingDelete !== null"
      variant="danger"
      :title="$t('admin-sla-delete-confirm-title')"
      :message="confirmDeleteMessage"
      :confirm-label="$t('admin-sla-delete')"
      @confirm="confirmDelete"
      @close="pendingDelete = null"
    />

    <!-- Calendar modal: create + edit. Holidays section only renders
         in edit mode because there's nothing to attach them to until
         the calendar has an id. -->
    <Modal
      :show="calendarOpen"
      :title="calendarModalTitle"
      size="lg"
      @close="closeCalendarModal"
    >
      <form class="flex flex-col gap-4" @submit.prevent="saveCalendar">
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <FormInput
            v-model="calendarDraft.name"
            size="sm"
            :label="$t('admin-sla-field-name')"
            :placeholder="$t('admin-sla-placeholder-name')"
          />
          <label class="flex flex-col gap-1.5">
            <span :class="FIELD_LABEL_CLASS">{{ $t('admin-sla-field-tz') }}</span>
            <SearchableDropdown
              v-model="calendarDraftTimezone"
              size="sm"
              :options="timezoneOptions"
              :placeholder="$t('admin-sla-placeholder-tz')"
              :search-placeholder="$t('admin-sla-tz-search-placeholder')"
              :empty-message="$t('admin-sla-tz-no-matches')"
            />
          </label>
        </div>

        <div class="flex flex-col gap-2 pt-2 border-t border-subtle">
          <span :class="FIELD_LABEL_CLASS">{{ $t('admin-sla-field-schedule') }}</span>
          <WeekScheduleEditor
            v-model="calendarDraft.schedule as WeekSchedule"
            @update:valid="(v: boolean) => (calendarScheduleValid = v)"
          />
        </div>

        <Checkbox
          :model-value="!!calendarDraft.is_default"
          size="sm"
          :label="$t('admin-sla-workspace-default')"
          @update:model-value="(v: boolean) => (calendarDraft.is_default = v)"
        />

        <!-- Holidays: only meaningful for a saved calendar. -->
        <div v-if="editingCalendar" class="flex flex-col gap-2 pt-2 border-t border-subtle">
          <div class="flex items-center justify-between gap-3">
            <span :class="FIELD_LABEL_CLASS">{{ $t('admin-sla-field-holidays') }}</span>
            <label class="flex items-center gap-2 text-[11px] text-tertiary">
              <span>{{ $t('admin-sla-holiday-import-label') }}</span>
              <select
                :value="importChoice"
                :class="FIELD_CLASS_SM"
                class="!w-auto"
                @change="(e) => handleImportChoice((e.target as HTMLSelectElement).value as CountryCode | '')"
              >
                <option value="">{{ $t('admin-sla-holiday-import-placeholder') }}</option>
                <option v-for="tpl in HOLIDAY_TEMPLATE_LIST" :key="tpl.code" :value="tpl.code">
                  {{ tpl.name }}
                </option>
              </select>
            </label>
          </div>
          <p
            v-if="importSummary"
            class="text-[11px] text-accent bg-accent/10 border border-accent/30 rounded px-2 py-1"
            role="status"
          >
            {{ importSummary }}
          </p>
          <ul
            v-if="editingHolidays.length > 0"
            class="flex flex-col divide-y divide-subtle border border-subtle rounded-md"
          >
            <li
              v-for="h in editingHolidays"
              :key="h.id"
              class="flex items-center gap-3 pl-3 pr-1.5 py-1 text-xs"
            >
              <span class="font-mono tabular-nums text-primary">{{ h.date }}</span>
              <span
                v-if="h.recurrence === 'annual'"
                class="text-[10px] uppercase tracking-wide font-semibold text-accent border border-accent/40 bg-accent/10 rounded px-1.5 py-0.5"
              >
                {{ $t('admin-sla-holiday-annual-badge') }}
              </span>
              <span class="flex-1 text-secondary truncate">{{ h.label ?? '' }}</span>
              <button
                type="button"
                class="p-1.5 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-md transition-colors"
                :aria-label="$t('admin-sla-holiday-remove-aria')"
                @click="removeHoliday(h.id)"
              >
                <Icon name="close" class="w-3.5 h-3.5" />
              </button>
            </li>
          </ul>
          <p v-else class="text-[11px] text-tertiary italic">
            {{ $t('admin-sla-holidays-empty-hint') }}
          </p>
          <div class="flex flex-col gap-2">
            <div class="flex flex-wrap items-end gap-2">
              <label class="flex flex-col gap-1">
                <span class="text-[10px] uppercase tracking-wide font-medium text-tertiary">
                  {{ $t('admin-sla-holiday-date') }}
                </span>
                <DatePicker
                  v-model="newHolidayDate"
                  size="sm"
                  :aria-label="$t('admin-sla-holiday-date')"
                />
              </label>
              <label class="flex flex-col gap-1 flex-1 min-w-[12rem]">
                <span class="text-[10px] uppercase tracking-wide font-medium text-tertiary">
                  {{ $t('admin-sla-holiday-label') }}
                </span>
                <input
                  v-model="newHolidayLabel"
                  type="text"
                  :placeholder="$t('admin-sla-holiday-placeholder')"
                  :class="FIELD_CLASS_SM"
                />
              </label>
              <Button
                type="button"
                size="sm"
                variant="secondary"
                :disabled="!newHolidayDate"
                @click="addHoliday"
              >
                {{ $t('admin-sla-holiday-add') }}
              </Button>
            </div>
            <Checkbox
              :model-value="newHolidayAnnual"
              size="sm"
              :label="$t('admin-sla-holiday-annual')"
              :description="$t('admin-sla-holiday-annual-hint')"
              @update:model-value="(v: boolean) => (newHolidayAnnual = v)"
            />
          </div>
        </div>

        <div class="flex items-center justify-end gap-2 pt-2 border-t border-subtle">
          <Button type="button" variant="secondary" size="sm" @click="closeCalendarModal">
            {{ $t('admin-sla-cancel') }}
          </Button>
          <Button
            type="submit"
            size="sm"
            :disabled="!calendarDraft.name.trim() || !calendarScheduleValid"
          >
            {{ $t('admin-sla-save') }}
          </Button>
        </div>
      </form>
    </Modal>

    <!-- Policy modal: create + edit. -->
    <Modal
      :show="policyOpen"
      :title="policyModalTitle"
      size="lg"
      @close="closePolicyModal"
    >
      <form class="flex flex-col gap-4" @submit.prevent="savePolicy">
        <FormInput
          v-model="policyDraft.name"
          size="sm"
          :label="$t('admin-sla-field-name')"
          :placeholder="$t('admin-sla-policy-name-placeholder')"
        />

        <!-- Conditions: which tickets does this policy match? -->
        <fieldset class="flex flex-col gap-2">
          <legend
            class="text-[11px] font-medium text-tertiary uppercase tracking-wide mb-1"
          >
            {{ $t('admin-sla-form-conditions-heading') }}
          </legend>
          <div class="grid grid-cols-1 sm:grid-cols-3 gap-3">
            <label class="flex flex-col gap-1.5">
              <span :class="FIELD_LABEL_CLASS">{{ $t('admin-sla-field-priority') }}</span>
              <select v-model="policyDraft.priority_filter" :class="FIELD_CLASS_SM">
                <option :value="null">{{ $t('admin-sla-priority-any') }}</option>
                <option value="low">{{ $t('admin-sla-priority-low') }}</option>
                <option value="medium">{{ $t('admin-sla-priority-medium') }}</option>
                <option value="high">{{ $t('admin-sla-priority-high') }}</option>
              </select>
            </label>
            <label class="flex flex-col gap-1.5">
              <span :class="FIELD_LABEL_CLASS">{{ $t('admin-sla-field-category') }}</span>
              <select
                v-model.number="policyDraft.category_id_filter"
                :class="FIELD_CLASS_SM"
              >
                <option :value="null">{{ $t('admin-sla-category-any') }}</option>
                <option v-for="opt in categoryOptions" :key="opt.value" :value="opt.value">
                  {{ opt.label }}
                </option>
              </select>
            </label>
            <label class="flex flex-col gap-1.5">
              <span :class="FIELD_LABEL_CLASS">
                {{ $t('admin-sla-field-assignee-group') }}
              </span>
              <select
                v-model.number="policyDraft.assignee_group_id_filter"
                :class="FIELD_CLASS_SM"
              >
                <option :value="null">{{ $t('admin-sla-assignee-group-any') }}</option>
                <option v-for="opt in groupOptions" :key="opt.value" :value="opt.value">
                  {{ opt.label }}
                </option>
              </select>
            </label>
          </div>
        </fieldset>

        <!-- Targets: what the engine computes for each match. -->
        <fieldset class="flex flex-col gap-2">
          <legend
            class="text-[11px] font-medium text-tertiary uppercase tracking-wide mb-1"
          >
            {{ $t('admin-sla-form-targets-heading') }}
          </legend>
          <div class="grid grid-cols-1 sm:grid-cols-3 gap-3">
            <label class="flex flex-col gap-1.5">
              <span :class="FIELD_LABEL_CLASS">{{ $t('admin-sla-field-calendar') }}</span>
              <select
                v-model.number="policyDraft.working_calendar_id"
                :class="FIELD_CLASS_SM"
              >
                <option :value="null">-</option>
                <option v-for="opt in calendarOptions" :key="opt.value" :value="opt.value">
                  {{ opt.label }}
                </option>
              </select>
            </label>
            <FormNumber
              :model-value="policyDraft.target_response_minutes ?? null"
              :label="$t('admin-sla-field-response')"
              size="sm"
              integer
              :min="0"
              @update:model-value="(v) => (policyDraft.target_response_minutes = v ?? undefined)"
            />
            <FormNumber
              :model-value="policyDraft.target_resolution_minutes ?? null"
              :label="$t('admin-sla-field-resolution')"
              size="sm"
              integer
              :min="0"
              @update:model-value="(v) => (policyDraft.target_resolution_minutes = v ?? undefined)"
            />
          </div>
        </fieldset>

        <Checkbox
          :model-value="!!policyDraft.is_default"
          size="sm"
          :label="$t('admin-sla-workspace-default')"
          @update:model-value="(v: boolean) => (policyDraft.is_default = v)"
        />

        <div class="flex items-center justify-end gap-2 pt-2 border-t border-subtle">
          <Button type="button" variant="secondary" size="sm" @click="closePolicyModal">
            {{ $t('admin-sla-cancel') }}
          </Button>
          <Button type="submit" size="sm" :disabled="!policyDraft.name.trim()">
            {{ $t('admin-sla-save') }}
          </Button>
        </div>
      </form>
    </Modal>
  </div>
</template>
