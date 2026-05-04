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
import {
  slaService,
  type SlaPolicy,
  type WorkingCalendar,
  type WorkingCalendarBody,
  type SlaPolicyBody,
} from '@/services/slaService'

const policies = ref<SlaPolicy[]>([])
const calendars = ref<WorkingCalendar[]>([])
const loading = ref(false)
const error = ref<string | null>(null)

async function load(): Promise<void> {
  loading.value = true
  error.value = null
  try {
    const [p, c] = await Promise.all([
      slaService.listPolicies(),
      slaService.listCalendars(),
    ])
    policies.value = p
    calendars.value = c
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load SLA config'
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
    error.value = e instanceof Error ? e.message : 'Create failed'
  }
}

async function deleteCalendar(id: number): Promise<void> {
  if (!window.confirm('Delete this calendar? Policies pointing at it will need a new calendar.')) return
  try {
    await slaService.deleteCalendar(id)
    calendars.value = calendars.value.filter((c) => c.id !== id)
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Delete failed'
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
    error.value = e instanceof Error ? e.message : 'Update failed'
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
    error.value = e instanceof Error ? e.message : 'Create failed'
  }
}

async function deletePolicy(id: number): Promise<void> {
  if (!window.confirm('Delete this policy?')) return
  try {
    await slaService.deletePolicy(id)
    policies.value = policies.value.filter((p) => p.id !== id)
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Delete failed'
  }
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
    error.value = e instanceof Error ? e.message : 'Update failed'
  }
}

function calendarName(id: number | null): string {
  if (id == null) return '—'
  return calendars.value.find((c) => c.id === id)?.name ?? `#${id}`
}

function fmtMinutes(m: number | null): string {
  if (m == null) return '—'
  if (m < 60) return `${m}m`
  if (m < 24 * 60) return `${(m / 60).toFixed(m % 60 === 0 ? 0 : 1)}h`
  return `${(m / (24 * 60)).toFixed(m % (24 * 60) === 0 ? 0 : 1)}d`
}
</script>

<template>
  <div class="flex flex-col h-full">
    <header class="flex items-center justify-between px-6 py-4 border-b border-subtle bg-app">
      <div>
        <h1 class="text-xl font-semibold text-primary">SLA</h1>
        <p class="text-xs text-tertiary mt-0.5">
          Working calendars and SLA policies feed the per-ticket SLA pill.
        </p>
      </div>
    </header>

    <div v-if="loading" class="flex-1 flex items-center justify-center text-tertiary text-sm">
      Loading…
    </div>
    <div v-else class="flex-1 min-h-0 overflow-y-auto p-6 grid gap-6" style="grid-template-columns: 1fr 1fr">
      <p v-if="error" class="col-span-2 text-sm text-rose-500">{{ error }}</p>

      <!-- Working calendars -->
      <section class="flex flex-col gap-3">
        <h2 class="text-sm font-semibold text-primary">Working calendars</h2>

        <table class="w-full text-xs border border-subtle rounded-md overflow-hidden">
          <thead class="bg-surface text-tertiary">
            <tr>
              <th class="text-left px-3 py-2 font-medium">Name</th>
              <th class="text-left px-3 py-2 font-medium">TZ</th>
              <th class="text-left px-3 py-2 font-medium">Default</th>
              <th class="px-3 py-2"></th>
            </tr>
          </thead>
          <tbody class="divide-y divide-subtle">
            <tr v-for="cal in calendars" :key="cal.id" class="bg-app">
              <td class="px-3 py-2 text-primary">{{ cal.name }}</td>
              <td class="px-3 py-2 text-secondary">{{ cal.timezone }}</td>
              <td class="px-3 py-2">
                <button
                  type="button"
                  class="text-[10px] uppercase tracking-wide font-semibold rounded px-1.5 py-0.5"
                  :class="cal.is_default ? 'bg-accent text-on-accent' : 'bg-surface-hover text-tertiary'"
                  @click="toggleCalendarDefault(cal)"
                >{{ cal.is_default ? 'Default' : 'Set default' }}</button>
              </td>
              <td class="px-3 py-2 text-right">
                <button
                  type="button"
                  class="text-[11px] text-tertiary hover:text-primary"
                  @click="deleteCalendar(cal.id)"
                >Delete</button>
              </td>
            </tr>
          </tbody>
        </table>

        <form
          class="rounded-md border border-subtle bg-app p-3 flex flex-col gap-2"
          @submit.prevent="createCalendar"
        >
          <h3 class="text-xs font-semibold text-secondary uppercase tracking-wide">New calendar</h3>
          <div class="grid grid-cols-2 gap-2">
            <label class="flex flex-col gap-1 text-[11px] text-tertiary">
              Name
              <input
                v-model="calendarDraft.name"
                type="text"
                class="bg-surface border border-subtle rounded-md text-sm px-2 py-1 text-primary"
                placeholder="EU support hours"
              />
            </label>
            <label class="flex flex-col gap-1 text-[11px] text-tertiary">
              Timezone
              <input
                v-model="calendarDraft.timezone"
                type="text"
                class="bg-surface border border-subtle rounded-md text-sm px-2 py-1 text-primary"
                placeholder="Europe/London"
              />
            </label>
          </div>
          <p class="text-[11px] text-tertiary italic">
            Schedule defaults to Mon-Fri 9-17. Edit by hand or expand here later.
          </p>
          <button
            type="submit"
            class="text-xs font-medium rounded-md px-3 py-1.5 bg-accent text-on-accent hover:opacity-90 disabled:opacity-50 self-end"
            :disabled="!calendarDraft.name.trim()"
          >Create</button>
        </form>
      </section>

      <!-- Policies -->
      <section class="flex flex-col gap-3">
        <h2 class="text-sm font-semibold text-primary">SLA policies</h2>

        <table class="w-full text-xs border border-subtle rounded-md overflow-hidden">
          <thead class="bg-surface text-tertiary">
            <tr>
              <th class="text-left px-3 py-2 font-medium">Name</th>
              <th class="text-left px-3 py-2 font-medium">Response</th>
              <th class="text-left px-3 py-2 font-medium">Resolution</th>
              <th class="text-left px-3 py-2 font-medium">Calendar</th>
              <th class="text-left px-3 py-2 font-medium">Default</th>
              <th class="px-3 py-2"></th>
            </tr>
          </thead>
          <tbody class="divide-y divide-subtle">
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
                  @click="deletePolicy(p.id)"
                >Delete</button>
              </td>
            </tr>
          </tbody>
        </table>

        <form
          class="rounded-md border border-subtle bg-app p-3 flex flex-col gap-2"
          @submit.prevent="createPolicy"
        >
          <h3 class="text-xs font-semibold text-secondary uppercase tracking-wide">New policy</h3>
          <div class="grid grid-cols-2 gap-2">
            <label class="flex flex-col gap-1 text-[11px] text-tertiary">
              Name
              <input
                v-model="policyDraft.name"
                type="text"
                class="bg-surface border border-subtle rounded-md text-sm px-2 py-1 text-primary"
                placeholder="Critical incidents"
              />
            </label>
            <label class="flex flex-col gap-1 text-[11px] text-tertiary">
              Calendar
              <select
                v-model.number="policyDraft.working_calendar_id"
                class="bg-surface border border-subtle rounded-md text-sm px-2 py-1 text-primary"
              >
                <option :value="null">—</option>
                <option v-for="opt in calendarOptions" :key="opt.value" :value="opt.value">
                  {{ opt.label }}
                </option>
              </select>
            </label>
            <label class="flex flex-col gap-1 text-[11px] text-tertiary">
              Response (minutes)
              <input
                v-model.number="policyDraft.target_response_minutes"
                type="number"
                min="0"
                class="bg-surface border border-subtle rounded-md text-sm px-2 py-1 text-primary"
              />
            </label>
            <label class="flex flex-col gap-1 text-[11px] text-tertiary">
              Resolution (minutes)
              <input
                v-model.number="policyDraft.target_resolution_minutes"
                type="number"
                min="0"
                class="bg-surface border border-subtle rounded-md text-sm px-2 py-1 text-primary"
              />
            </label>
            <label class="flex flex-col gap-1 text-[11px] text-tertiary">
              Priority filter
              <select
                v-model="policyDraft.priority_filter"
                class="bg-surface border border-subtle rounded-md text-sm px-2 py-1 text-primary"
              >
                <option :value="null">Any</option>
                <option value="low">low</option>
                <option value="medium">medium</option>
                <option value="high">high</option>
              </select>
            </label>
            <label class="flex items-center gap-2 text-[11px] text-tertiary mt-5">
              <input v-model="policyDraft.is_default" type="checkbox" />
              Workspace default
            </label>
          </div>
          <button
            type="submit"
            class="text-xs font-medium rounded-md px-3 py-1.5 bg-accent text-on-accent hover:opacity-90 disabled:opacity-50 self-end"
            :disabled="!policyDraft.name.trim()"
          >Create</button>
        </form>
      </section>
    </div>
  </div>
</template>
