<script setup lang="ts">
/**
 * SLA admin: list + edit working calendars and SLA policies.
 *
 * The two tables sit side-by-side because policies depend on
 * calendars (a policy with no calendar can't compute a target).
 * Edit happens inline in the table — minimum-viable affordance
 * to get the engine usable. A richer modal-driven editor lands
 * later if real workflows demand it.
 *
 * Holidays are not editable here yet; admins who need to add a
 * holiday today patch the row directly. The backend already
 * honours the table.
 */
import { computed, onMounted, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import {
  slaService,
  type SlaPolicy,
  type WorkingCalendar,
  type WorkingCalendarBody,
  type SlaPolicyBody,
} from '@/services/slaService'
import { categoryService } from '@/services/categoryService'
import type { TicketCategory } from '@/types/category'
import Checkbox from '@/components/common/Checkbox.vue'
import Button from '@/components/common/Button.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import Skeleton from '@/components/common/Skeleton.vue'
import SkeletonBar from '@/components/common/SkeletonBar.vue'

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

const policies = ref<SlaPolicy[]>([])
const calendars = ref<WorkingCalendar[]>([])
const categories = ref<TicketCategory[]>([])
const loading = ref(false)
const error = ref<string | null>(null)

// Single pending-confirm state covers both calendar + policy
// deletes so the template renders one ConfirmModal instance.
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

async function load(): Promise<void> {
  loading.value = true
  error.value = null
  try {
    const [p, c, cats] = await Promise.all([
      slaService.listPolicies(),
      slaService.listCalendars(),
      categoryService.getCategories(),
    ])
    policies.value = p
    calendars.value = c
    categories.value = cats
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('admin-sla-error-load')
  } finally {
    loading.value = false
  }
}

onMounted(load)

// ---------------- Calendar create form ----------------
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
    calendars.value.push(created)
    calendarDraft.value = {
      name: '',
      timezone: 'UTC',
      schedule: { ...calendarDraft.value.schedule },
      is_default: false,
    }
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
    calendars.value = calendars.value.filter((c) => c.id !== id)
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
    const idx = calendars.value.findIndex((c) => c.id === cal.id)
    if (idx >= 0) calendars.value[idx] = updated
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('admin-sla-error-update')
  }
}

// ---------------- Policy create form ----------------
const policyDraft = ref<SlaPolicyBody>({
  name: '',
  target_response_minutes: 60,
  target_resolution_minutes: 24 * 60,
  working_calendar_id: null,
  priority_filter: null,
  category_id_filter: null,
  is_default: false,
})

const calendarOptions = computed(() =>
  calendars.value.map((c) => ({ value: c.id, label: c.name })),
)

const categoryOptions = computed(() =>
  categories.value.map((c) => ({ value: c.id, label: c.name })),
)

async function createPolicy(): Promise<void> {
  if (!policyDraft.value.name.trim()) return
  try {
    const created = await slaService.createPolicy(policyDraft.value)
    policies.value.push(created)
    policyDraft.value = {
      name: '',
      target_response_minutes: 60,
      target_resolution_minutes: 24 * 60,
      working_calendar_id: null,
      priority_filter: null,
      category_id_filter: null,
      is_default: false,
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('admin-sla-error-create')
  }
}

// Confirm wording for policy delete calls out the side effect: any
// tickets the policy currently covers stop having an SLA. Without
// this the operator might delete "Standard SLA" thinking they can
// recreate it later and not realise tickets in flight lose their
// pill until the new policy lands. (Wording lives in the FTL key.)
function requestDeletePolicy(p: SlaPolicy): void {
  pendingDelete.value = { kind: 'policy', id: p.id, name: p.name }
}

async function deletePolicy(id: number): Promise<void> {
  try {
    await slaService.deletePolicy(id)
    policies.value = policies.value.filter((p) => p.id !== id)
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
      is_default: p.is_default,
      ...patch,
    })
    const idx = policies.value.findIndex((x) => x.id === p.id)
    if (idx >= 0) policies.value[idx] = updated
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
      v-if="loading"
      :label="$t('admin-sla-loading')"
      class="flex-1 min-h-0 overflow-hidden p-6 grid gap-6"
      style="grid-template-columns: 1fr 1fr"
    >
      <!-- Calendars + policies mirror the live two-section layout
           below so the cutover doesn't reflow when data arrives. -->
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
    <div v-else class="flex-1 min-h-0 overflow-y-auto p-6 grid gap-6" style="grid-template-columns: 1fr 1fr">
      <p v-if="error" class="col-span-2 text-sm text-status-error">{{ error }}</p>

      <!-- Working calendars -->
      <section class="flex flex-col gap-3">
        <h2 class="text-sm font-semibold text-primary">{{ $t('admin-sla-calendars-heading') }}</h2>

        <div class="overflow-x-auto">
        <table class="w-full min-w-[480px] text-xs border border-subtle rounded-md overflow-hidden">
          <thead class="bg-surface text-tertiary">
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
                  :class="cal.is_default ? 'bg-accent text-on-accent' : 'bg-surface-hover text-tertiary'"
                  @click="toggleCalendarDefault(cal)"
                >{{ cal.is_default ? $t('admin-sla-default-badge') : $t('admin-sla-set-default') }}</button>
              </td>
              <td class="px-3 py-2 text-right">
                <button
                  type="button"
                  class="text-[11px] text-tertiary hover:text-primary"
                  @click="requestDeleteCalendar(cal)"
                >{{ $t('admin-sla-delete') }}</button>
              </td>
            </tr>
          </tbody>
        </table>
        </div>

        <form
          class="rounded-md border border-subtle bg-app p-3 flex flex-col gap-2"
          @submit.prevent="createCalendar"
        >
          <h3 class="text-xs font-semibold text-secondary uppercase tracking-wide">{{ $t('admin-sla-new-calendar-heading') }}</h3>
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
            <label class="flex flex-col gap-1 text-[11px] text-tertiary">
              {{ $t('admin-sla-field-name') }}
              <input
                v-model="calendarDraft.name"
                type="text"
                class="bg-surface border border-subtle rounded-md text-sm px-2 py-1 text-primary"
                :placeholder="$t('admin-sla-placeholder-name')"
              />
            </label>
            <label class="flex flex-col gap-1 text-[11px] text-tertiary">
              {{ $t('admin-sla-field-tz') }}
              <input
                v-model="calendarDraft.timezone"
                type="text"
                class="bg-surface border border-subtle rounded-md text-sm px-2 py-1 text-primary"
                :placeholder="$t('admin-sla-placeholder-tz')"
              />
            </label>
          </div>
          <p class="text-[11px] text-tertiary italic">
            {{ $t('admin-sla-schedule-hint') }}
          </p>
          <Button type="submit" size="sm" class="self-end" :disabled="!calendarDraft.name.trim()">
            {{ $t('admin-sla-create') }}
          </Button>
        </form>
      </section>

      <!-- Policies -->
      <section class="flex flex-col gap-3">
        <h2 class="text-sm font-semibold text-primary">{{ $t('admin-sla-policies-heading') }}</h2>

        <div class="overflow-x-auto">
        <table class="w-full min-w-[480px] text-xs border border-subtle rounded-md overflow-hidden">
          <thead class="bg-surface text-tertiary">
            <tr>
              <th class="text-left px-3 py-2 font-medium">{{ $t('admin-sla-col-name') }}</th>
              <th class="text-left px-3 py-2 font-medium">{{ $t('admin-sla-col-response') }}</th>
              <th class="text-left px-3 py-2 font-medium">{{ $t('admin-sla-col-resolution') }}</th>
              <th class="text-left px-3 py-2 font-medium">{{ $t('admin-sla-col-calendar') }}</th>
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
              <td class="px-3 py-2 text-secondary">{{ fmtMinutes(p.target_response_minutes) }}</td>
              <td class="px-3 py-2 text-secondary">{{ fmtMinutes(p.target_resolution_minutes) }}</td>
              <td class="px-3 py-2 text-secondary">{{ calendarName(p.working_calendar_id) }}</td>
              <td class="px-3 py-2">
                <button
                  type="button"
                  class="text-[10px] uppercase tracking-wide font-semibold rounded px-1.5 py-0.5"
                  :class="p.is_default ? 'bg-accent text-on-accent' : 'bg-surface-hover text-tertiary'"
                  @click="patchPolicy(p, { is_default: !p.is_default })"
                >{{ p.is_default ? 'Default' : 'Set default' }}</button>
              </td>
              <td class="px-3 py-2 text-right">
                <button
                  type="button"
                  class="text-[11px] text-tertiary hover:text-primary"
                  @click="requestDeletePolicy(p)"
                >{{ $t('admin-sla-delete') }}</button>
              </td>
            </tr>
          </tbody>
        </table>
        </div>

        <form
          class="rounded-md border border-subtle bg-app p-3 flex flex-col gap-2"
          @submit.prevent="createPolicy"
        >
          <h3 class="text-xs font-semibold text-secondary uppercase tracking-wide">{{ $t('admin-sla-new-policy-heading') }}</h3>
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
            <label class="flex flex-col gap-1 text-[11px] text-tertiary">
              {{ $t('admin-sla-field-name') }}
              <input
                v-model="policyDraft.name"
                type="text"
                class="bg-surface border border-subtle rounded-md text-sm px-2 py-1 text-primary"
                :placeholder="$t('admin-sla-policy-name-placeholder')"
              />
            </label>
            <label class="flex flex-col gap-1 text-[11px] text-tertiary">
              {{ $t('admin-sla-field-calendar') }}
              <select
                v-model.number="policyDraft.working_calendar_id"
                class="bg-surface border border-subtle rounded-md text-sm px-2 py-1 text-primary"
              >
                <option :value="null">-</option>
                <option v-for="opt in calendarOptions" :key="opt.value" :value="opt.value">
                  {{ opt.label }}
                </option>
              </select>
            </label>
            <label class="flex flex-col gap-1 text-[11px] text-tertiary">
              {{ $t('admin-sla-field-response') }}
              <input
                v-model.number="policyDraft.target_response_minutes"
                type="number"
                min="0"
                class="bg-surface border border-subtle rounded-md text-sm px-2 py-1 text-primary"
              />
            </label>
            <label class="flex flex-col gap-1 text-[11px] text-tertiary">
              {{ $t('admin-sla-field-resolution') }}
              <input
                v-model.number="policyDraft.target_resolution_minutes"
                type="number"
                min="0"
                class="bg-surface border border-subtle rounded-md text-sm px-2 py-1 text-primary"
              />
            </label>
            <label class="flex flex-col gap-1 text-[11px] text-tertiary">
              {{ $t('admin-sla-field-priority') }}
              <select
                v-model="policyDraft.priority_filter"
                class="bg-surface border border-subtle rounded-md text-sm px-2 py-1 text-primary"
              >
                <option :value="null">{{ $t('admin-sla-priority-any') }}</option>
                <option value="low">{{ $t('admin-sla-priority-low') }}</option>
                <option value="medium">{{ $t('admin-sla-priority-medium') }}</option>
                <option value="high">{{ $t('admin-sla-priority-high') }}</option>
              </select>
            </label>
            <label class="flex flex-col gap-1 text-[11px] text-tertiary">
              {{ $t('admin-sla-field-category') }}
              <select
                v-model.number="policyDraft.category_id_filter"
                class="bg-surface border border-subtle rounded-md text-sm px-2 py-1 text-primary"
              >
                <option :value="null">{{ $t('admin-sla-category-any') }}</option>
                <option v-for="opt in categoryOptions" :key="opt.value" :value="opt.value">
                  {{ opt.label }}
                </option>
              </select>
            </label>
            <Checkbox
              :model-value="!!policyDraft.is_default"
              size="sm"
              :label="$t('admin-sla-workspace-default')"
              class="mt-5"
              @update:model-value="(v: boolean) => (policyDraft.is_default = v)"
            />
          </div>
          <Button type="submit" size="sm" class="self-end" :disabled="!policyDraft.name.trim()">
            {{ $t('admin-sla-create') }}
          </Button>
        </form>
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
