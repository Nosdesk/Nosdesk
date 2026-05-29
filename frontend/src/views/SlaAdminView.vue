<script setup lang="ts">
/**
 * SLA admin: list + edit working calendars and SLA policies.
 *
 * Two columns side by side because policies depend on calendars (a
 * policy with no calendar can't compute a target). Each column is a
 * SectionCard so the chrome matches every other card-with-header in
 * the app, and create lives inline below the list (no modal — admins
 * write a calendar or policy a handful of times then forget it).
 *
 * Reads go through Pinia Colada so revisits paint from cache and
 * background revalidations stay silent; mutations push the new row
 * into the cache directly rather than refetching. A first-load
 * skeleton mirrors the live two-column shape so the cutover doesn't
 * reflow when data arrives.
 *
 * The policy form is grouped into Conditions (which tickets the
 * policy matches) and Targets (the times it sets) so the admin reads
 * top to bottom in the same order the engine evaluates: filter, then
 * compute.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery, useQueryCache } from '@pinia/colada'
import {
  slaService,
  type SlaPolicy,
  type WorkingCalendar,
  type WorkingCalendarBody,
  type SlaPolicyBody,
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
import Skeleton from '@/components/common/Skeleton.vue'
import SkeletonBar from '@/components/common/SkeletonBar.vue'

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

// ---------------- Query keys + queries ----------------
// Cache scoping mirrors WebhooksView: tuple keys, one query per
// resource, computed views that fall back to [] so the template
// never sees undefined.
const POLICIES_KEY = ['sla', 'policies'] as const
const CALENDARS_KEY = ['sla', 'calendars'] as const
const CATEGORIES_KEY = ['categories'] as const
const GROUPS_KEY = ['groups'] as const

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

const policies = computed<SlaPolicy[]>(() => policiesQuery.data.value ?? [])
const calendars = computed<WorkingCalendar[]>(() => calendarsQuery.data.value ?? [])
const categories = computed<TicketCategory[]>(() => categoriesQuery.data.value ?? [])
const groups = computed<GroupWithMemberCount[]>(() => groupsQuery.data.value ?? [])

// Skeleton only on a genuine cache miss — revisits paint instantly.
// Calendars + policies decide first-paint together because each
// section renders independently; revalidations of categories or
// groups don't reset the skeleton.
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
// Single pending-confirm state covers both calendar + policy deletes
// so the template renders one ConfirmModal instance.
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

// ---------------- Calendar create + mutate ----------------
const calendarDraft = ref<WorkingCalendarBody>({
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
})

async function createCalendar(): Promise<void> {
  if (!calendarDraft.value.name.trim()) return
  try {
    const created = await slaService.createCalendar(calendarDraft.value)
    queryCache.setQueryData(CALENDARS_KEY, [...calendars.value, created])
    calendarDraft.value = {
      name: '',
      timezone: 'UTC',
      schedule: { ...calendarDraft.value.schedule },
      is_default: false,
    }
    error.value = null
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('admin-sla-error-create')
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

// ---------------- Policy create + mutate ----------------
const policyDraft = ref<SlaPolicyBody>({
  name: '',
  target_response_minutes: 60,
  target_resolution_minutes: 24 * 60,
  working_calendar_id: null,
  priority_filter: null,
  category_id_filter: null,
  assignee_group_id_filter: null,
  is_default: false,
})

const calendarOptions = computed(() =>
  calendars.value.map((c) => ({ value: c.id, label: c.name })),
)
const categoryOptions = computed(() =>
  categories.value.map((c) => ({ value: c.id, label: c.name })),
)
const groupOptions = computed(() =>
  groups.value.map((g) => ({ value: g.id, label: g.name })),
)

async function createPolicy(): Promise<void> {
  if (!policyDraft.value.name.trim()) return
  try {
    const created = await slaService.createPolicy(policyDraft.value)
    queryCache.setQueryData(POLICIES_KEY, [...policies.value, created])
    policyDraft.value = {
      name: '',
      target_response_minutes: 60,
      target_resolution_minutes: 24 * 60,
      working_calendar_id: null,
      priority_filter: null,
      category_id_filter: null,
      assignee_group_id_filter: null,
      is_default: false,
    }
    error.value = null
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('admin-sla-error-create')
  }
}

// Confirm wording for policy delete calls out the side effect: any
// tickets the policy currently covers stop having an SLA. Without
// this the operator might delete "Standard SLA" thinking they can
// recreate it later and not realise tickets in flight lose their
// pill until the new policy lands.
function requestDeletePolicy(p: SlaPolicy): void {
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

async function patchPolicy(p: SlaPolicy, patch: Partial<SlaPolicyBody>): Promise<void> {
  try {
    const updated = await slaService.updatePolicy(p.id, {
      name: p.name,
      target_response_minutes: p.target_response_minutes,
      target_resolution_minutes: p.target_resolution_minutes,
      working_calendar_id: p.working_calendar_id,
      priority_filter: p.priority_filter,
      category_id_filter: p.category_id_filter,
      assignee_group_id_filter: p.assignee_group_id_filter,
      is_default: p.is_default,
      ...patch,
    })
    queryCache.setQueryData(
      POLICIES_KEY,
      policies.value.map((x) => (x.id === p.id ? updated : x)),
    )
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('admin-sla-error-update')
  }
}

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
// and the bare number/optional-string <input> reads as the same
// control. FormInput is string-only and required, so we drop down to
// the native element for numeric and nullable-optional fields. If a
// FormSelect/FormNumberInput lands later this constant disappears.
const FIELD_CLASS_SM =
  'w-full bg-surface-alt border border-subtle rounded-lg text-primary px-3 py-1.5 text-sm ' +
  'placeholder-tertiary transition-colors ' +
  'focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:border-accent ' +
  'disabled:opacity-50 disabled:cursor-not-allowed'

// Match FormInput's label styling exactly so the inline labels above
// bare fields read identical to FormInput's own.
const FIELD_LABEL_CLASS = 'text-xs font-medium text-tertiary uppercase tracking-wide'
</script>

<template>
  <div class="flex flex-col h-full">
    <header class="flex items-center justify-between px-6 py-4 border-b border-subtle bg-app">
      <div>
        <h1 class="text-xl font-semibold text-primary">{{ $t('admin-sla-title') }}</h1>
        <p class="text-xs text-tertiary mt-0.5 max-w-2xl">
          {{ $t('admin-sla-description') }}
        </p>
      </div>
    </header>

    <Skeleton
      v-if="isFirstLoad"
      :label="$t('admin-sla-loading')"
      class="flex-1 min-h-0 overflow-hidden p-6 grid gap-6"
      style="grid-template-columns: 1fr 1fr"
    >
      <!-- Mirror the live two-section layout so the cutover doesn't
           reflow when data arrives. -->
      <section v-for="col in 2" :key="col" class="flex flex-col gap-3">
        <SkeletonBar class="h-4 w-32" />
        <div class="border border-subtle rounded-md overflow-hidden">
          <SkeletonBar class="h-7 w-full" />
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
      </section>
    </Skeleton>

    <div
      v-else
      class="flex-1 min-h-0 overflow-y-auto p-6 grid gap-6"
      style="grid-template-columns: 1fr 1fr"
    >
      <p v-if="liveError" class="col-span-2 text-sm text-status-error">{{ liveError }}</p>

      <!-- Working calendars -->
      <section class="flex flex-col gap-3">
        <SectionCard content-padding="">
          <template #title>{{ $t('admin-sla-calendars-heading') }}</template>
          <div class="overflow-x-auto">
            <table class="w-full min-w-[480px] text-xs">
              <thead class="bg-surface-alt text-tertiary">
                <tr>
                  <th class="text-left px-3 py-2 font-medium">{{ $t('admin-sla-col-name') }}</th>
                  <th class="text-left px-3 py-2 font-medium">{{ $t('admin-sla-col-tz') }}</th>
                  <th class="text-left px-3 py-2 font-medium">{{ $t('admin-sla-col-default') }}</th>
                  <th class="px-3 py-2"></th>
                </tr>
              </thead>
              <tbody class="divide-y divide-subtle">
                <tr v-if="!calendars.length" class="bg-app">
                  <td colspan="4" class="px-3 py-4 text-tertiary text-center">
                    {{ $t('admin-sla-no-calendars-hint') }}
                  </td>
                </tr>
                <tr v-for="cal in calendars" :key="cal.id" class="bg-app">
                  <td class="px-3 py-2 text-primary">{{ cal.name }}</td>
                  <td class="px-3 py-2 text-secondary">{{ cal.timezone }}</td>
                  <td class="px-3 py-2">
                    <button
                      type="button"
                      class="text-[10px] uppercase tracking-wide font-semibold rounded px-1.5 py-0.5"
                      :class="
                        cal.is_default
                          ? 'bg-accent text-on-accent'
                          : 'bg-surface-hover text-tertiary'
                      "
                      @click="toggleCalendarDefault(cal)"
                    >
                      {{
                        cal.is_default
                          ? $t('admin-sla-default-badge')
                          : $t('admin-sla-set-default')
                      }}
                    </button>
                  </td>
                  <td class="px-3 py-2 text-right">
                    <button
                      type="button"
                      class="text-[11px] text-tertiary hover:text-primary"
                      @click="requestDeleteCalendar(cal)"
                    >
                      {{ $t('admin-sla-delete') }}
                    </button>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </SectionCard>

        <SectionCard>
          <template #title>{{ $t('admin-sla-new-calendar-heading') }}</template>
          <form class="flex flex-col gap-3" @submit.prevent="createCalendar">
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <FormInput
                v-model="calendarDraft.name"
                size="sm"
                :label="$t('admin-sla-field-name')"
                :placeholder="$t('admin-sla-placeholder-name')"
              />
              <label class="flex flex-col gap-1.5">
                <span :class="FIELD_LABEL_CLASS">{{ $t('admin-sla-field-tz') }}</span>
                <input
                  v-model="calendarDraft.timezone"
                  type="text"
                  :placeholder="$t('admin-sla-placeholder-tz')"
                  :class="FIELD_CLASS_SM"
                />
              </label>
            </div>
            <p class="text-[11px] text-tertiary italic">
              {{ $t('admin-sla-schedule-hint') }}
            </p>
            <Button
              type="submit"
              size="sm"
              class="self-end"
              :disabled="!calendarDraft.name.trim()"
            >
              {{ $t('admin-sla-create') }}
            </Button>
          </form>
        </SectionCard>
      </section>

      <!-- Policies -->
      <section class="flex flex-col gap-3">
        <SectionCard content-padding="">
          <template #title>{{ $t('admin-sla-policies-heading') }}</template>
          <div class="overflow-x-auto">
            <table class="w-full min-w-[480px] text-xs">
              <thead class="bg-surface-alt text-tertiary">
                <tr>
                  <th class="text-left px-3 py-2 font-medium">{{ $t('admin-sla-col-name') }}</th>
                  <th class="text-left px-3 py-2 font-medium">
                    {{ $t('admin-sla-col-response') }}
                  </th>
                  <th class="text-left px-3 py-2 font-medium">
                    {{ $t('admin-sla-col-resolution') }}
                  </th>
                  <th class="text-left px-3 py-2 font-medium">
                    {{ $t('admin-sla-col-calendar') }}
                  </th>
                  <th class="text-left px-3 py-2 font-medium">{{ $t('admin-sla-col-default') }}</th>
                  <th class="px-3 py-2"></th>
                </tr>
              </thead>
              <tbody class="divide-y divide-subtle">
                <tr v-if="!policies.length" class="bg-app">
                  <td colspan="6" class="px-3 py-4 text-tertiary text-center">
                    {{ $t('admin-sla-no-policies-hint') }}
                  </td>
                </tr>
                <tr v-for="p in policies" :key="p.id" class="bg-app">
                  <td class="px-3 py-2 text-primary">{{ p.name }}</td>
                  <td class="px-3 py-2 text-secondary">
                    {{ fmtMinutes(p.target_response_minutes) }}
                  </td>
                  <td class="px-3 py-2 text-secondary">
                    {{ fmtMinutes(p.target_resolution_minutes) }}
                  </td>
                  <td class="px-3 py-2 text-secondary">
                    {{ calendarName(p.working_calendar_id) }}
                  </td>
                  <td class="px-3 py-2">
                    <button
                      type="button"
                      class="text-[10px] uppercase tracking-wide font-semibold rounded px-1.5 py-0.5"
                      :class="
                        p.is_default
                          ? 'bg-accent text-on-accent'
                          : 'bg-surface-hover text-tertiary'
                      "
                      @click="patchPolicy(p, { is_default: !p.is_default })"
                    >
                      {{
                        p.is_default
                          ? $t('admin-sla-default-badge')
                          : $t('admin-sla-set-default')
                      }}
                    </button>
                  </td>
                  <td class="px-3 py-2 text-right">
                    <button
                      type="button"
                      class="text-[11px] text-tertiary hover:text-primary"
                      @click="requestDeletePolicy(p)"
                    >
                      {{ $t('admin-sla-delete') }}
                    </button>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </SectionCard>

        <SectionCard>
          <template #title>{{ $t('admin-sla-new-policy-heading') }}</template>
          <form class="flex flex-col gap-4" @submit.prevent="createPolicy">
            <FormInput
              v-model="policyDraft.name"
              size="sm"
              :label="$t('admin-sla-field-name')"
              :placeholder="$t('admin-sla-policy-name-placeholder')"
            />

            <!-- Conditions: filters the matcher reads. Top section
                 because admins decide who the policy is *for* before
                 they decide what targets it gets. -->
            <fieldset class="flex flex-col gap-2">
              <legend
                class="text-[11px] font-medium text-tertiary uppercase tracking-wide mb-1"
              >
                {{ $t('admin-sla-form-conditions-heading') }}
              </legend>
              <div class="grid grid-cols-1 sm:grid-cols-3 gap-3">
                <label class="flex flex-col gap-1.5">
                  <span :class="FIELD_LABEL_CLASS">
                    {{ $t('admin-sla-field-priority') }}
                  </span>
                  <select v-model="policyDraft.priority_filter" :class="FIELD_CLASS_SM">
                    <option :value="null">{{ $t('admin-sla-priority-any') }}</option>
                    <option value="low">{{ $t('admin-sla-priority-low') }}</option>
                    <option value="medium">{{ $t('admin-sla-priority-medium') }}</option>
                    <option value="high">{{ $t('admin-sla-priority-high') }}</option>
                  </select>
                </label>
                <label class="flex flex-col gap-1.5">
                  <span :class="FIELD_LABEL_CLASS">
                    {{ $t('admin-sla-field-category') }}
                  </span>
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
                  <span :class="FIELD_LABEL_CLASS">
                    {{ $t('admin-sla-field-calendar') }}
                  </span>
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
                <label class="flex flex-col gap-1.5">
                  <span :class="FIELD_LABEL_CLASS">{{ $t('admin-sla-field-response') }}</span>
                  <input
                    v-model.number="policyDraft.target_response_minutes"
                    type="number"
                    min="0"
                    :class="FIELD_CLASS_SM"
                  />
                </label>
                <label class="flex flex-col gap-1.5">
                  <span :class="FIELD_LABEL_CLASS">{{ $t('admin-sla-field-resolution') }}</span>
                  <input
                    v-model.number="policyDraft.target_resolution_minutes"
                    type="number"
                    min="0"
                    :class="FIELD_CLASS_SM"
                  />
                </label>
              </div>
            </fieldset>

            <div class="flex items-center justify-between gap-3">
              <Checkbox
                :model-value="!!policyDraft.is_default"
                size="sm"
                :label="$t('admin-sla-workspace-default')"
                @update:model-value="(v: boolean) => (policyDraft.is_default = v)"
              />
              <Button type="submit" size="sm" :disabled="!policyDraft.name.trim()">
                {{ $t('admin-sla-create') }}
              </Button>
            </div>
          </form>
        </SectionCard>
      </section>
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
  </div>
</template>
