<script setup lang="ts">
/**
 * Right-side preview panel for the tickets split view.
 *
 * Reads the selected ticket from the sync pool (already pooled
 * for the table — no extra fetch). Triage fields (title, status,
 * priority, assignee, due date) are editable inline via the same
 * pickers the ticket detail sidebar uses; writes go through the
 * sync store so the table row updates optimistically.
 *
 * Visual structure:
 *   - Top strip: id breadcrumb + close + open shortcuts
 *   - Title block: editable title + priority / SLA pills
 *   - PROPERTIES section: status, people, due date, cycle, category
 *   - SLA section (when present): time bar + target time
 *   - ACTIVITY section: created + last activity
 *   - DEVICES section (when present): leading device + count
 */
import { computed, onMounted, ref, toRef, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'
import DatePicker from '@/components/common/DatePicker.vue'
import CustomDropdown from '@/components/ticketComponents/CustomDropdown.vue'
import UserPicker from '@/components/ticketComponents/UserPicker.vue'
import UserCell from '@/components/views/UserCell.vue'
import { useReference } from '@/sync/composables'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import { useWorkflowStatesStore } from '@/stores/workflowStates'
import {
  buildWorkflowDropdownOptions,
  isCategoryHeaderValue,
} from '@/types/workflow'
import { useSlaState, useSlaTimers, type SlaState } from '@/composables/useSlaState'
import {
  formatCleanRelativeTime,
  formatDateTime,
} from '@/utils/dateUtils'
import type { CardData, Priority } from '@/sync/views/types'
import {
  buildPriorityDropdownOptions,
  priorityForBadge,
  priorityToneClass,
} from '@/utils/priorityHelpers'

const props = defineProps<{
  card: CardData | null
}>()

const emit = defineEmits<{
  (e: 'open', id: number): void
  (e: 'close'): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const ticketsStore = useSyncTicketsStore()
const workflowStatesStore = useWorkflowStatesStore()

onMounted(() => {
  void workflowStatesStore.load()
})

const titleDraft = ref('')
watch(
  () => props.card?.id,
  () => {
    titleDraft.value = props.card?.title ?? ''
  },
  { immediate: true },
)

const assigneeUser = useReference<{
  uuid: string
  name: string
  email?: string
  avatar_thumb?: string | null
  avatar_url?: string | null
}>('user', () => props.card?.assignee_uuid ?? null)

const workflowDropdownOptions = computed(() =>
  buildWorkflowDropdownOptions(
    workflowStatesStore.byCategory,
    workflowStatesStore.loaded,
    workflowStatesStore.states.length,
  ),
)

const priorityOptions = computed(() => buildPriorityDropdownOptions(t))

const workflowDropdownValue = computed(() =>
  props.card ? String(props.card.workflow_state.id) : '',
)

const selectedPriority = computed(() => props.card?.priority ?? 'none')

const dueDateValue = computed({
  get(): string {
    const raw = props.card?.due_date
    return raw ? raw.slice(0, 10) : ''
  },
  set(value: string) {
    if (!props.card) return
    void updateDueDate(value ? `${value}T00:00:00` : null)
  },
})

function relativeOrAbsolute(iso: string): string {
  return formatCleanRelativeTime(iso)
}

function fullDateTime(iso: string | null | undefined): string {
  return formatDateTime(iso)
}

const slaState = useSlaState(toRef(props, 'card'))
const slaTimers = useSlaTimers(toRef(props, 'card'))

// What the SLA section renders: both timers labelled when the policy
// has both targets configured, otherwise the one timer unlabeled.
// The labelKey is resolved by the template's $t so we don't have to
// pull in useFluent here. Computed so the template loop renders one
// shape regardless of how many timers exist; the empty-array case
// naturally hides the section.
const slaRows = computed<Array<{ labelKey: string | null; state: SlaState }>>(() => {
  const { response, resolution } = slaTimers.value
  if (response && resolution) {
    return [
      { labelKey: 'views-ticket-preview-sla-response', state: response },
      { labelKey: 'views-ticket-preview-sla-resolution', state: resolution },
    ]
  }
  const only = response ?? resolution
  return only ? [{ labelKey: null, state: only }] : []
})

function onOpen(): void {
  if (props.card) emit('open', props.card.id)
}

async function commitTitle(): Promise<void> {
  if (!props.card) return
  const next = titleDraft.value.trim()
  if (!next || next === props.card.title) {
    titleDraft.value = props.card.title
    return
  }
  await ticketsStore.patchTitle(props.card.id, next)
}

async function updateStatus(value: string): Promise<void> {
  if (!props.card || isCategoryHeaderValue(value)) return
  const stateId = Number(value)
  if (!Number.isFinite(stateId)) return
  const state = workflowStatesStore.findById(stateId)
  if (!state) return
  await ticketsStore.moveToWorkflowState(props.card.id, {
    id: state.id,
    name: state.name,
    category: state.category,
    color: state.color,
  })
}

async function updatePriority(value: string): Promise<void> {
  if (!props.card) return
  await ticketsStore.patchKanbanFields(props.card.id, {
    priority: value as Priority,
  })
}

async function updateAssignee(uuid: string): Promise<void> {
  if (!props.card) return
  await ticketsStore.patchKanbanFields(props.card.id, {
    assignee_uuid: uuid || null,
  })
}

async function updateDueDate(iso: string | null): Promise<void> {
  if (!props.card) return
  await ticketsStore.patchKanbanFields(props.card.id, { due_date: iso })
}
</script>

<template>
  <aside
    class="flex flex-col h-full bg-surface min-w-0"
    :aria-label="$t('views-ticket-preview-aria')"
  >
    <!-- Empty state: no selection -->
    <div
      v-if="!card"
      class="flex-1 flex flex-col items-center justify-center text-tertiary px-6 text-center"
    >
      <div class="w-12 h-12 rounded-full bg-surface-hover flex items-center justify-center mb-4">
        <Icon name="document" class="w-5 h-5 text-tertiary/60" />
      </div>
      <p class="text-sm font-medium text-secondary mb-2">{{ $t('views-ticket-preview-empty-title') }}</p>
      <p class="text-xs leading-relaxed max-w-[16rem]">
        {{ $t('views-ticket-preview-empty-prefix') }}
        <kbd class="text-[10px] font-mono px-1.5 py-0.5 bg-surface-hover rounded border border-subtle">↑</kbd>
        <kbd class="text-[10px] font-mono px-1.5 py-0.5 bg-surface-hover rounded border border-subtle ml-1">↓</kbd>
        {{ $t('views-ticket-preview-empty-suffix') }}
      </p>
    </div>

    <Transition name="preview-fade" mode="out-in">
      <div
        v-if="card"
        :key="card.id"
        class="flex flex-col flex-1 min-h-0"
      >
        <!-- Top strip: breadcrumb + actions -->
        <header
          class="flex items-center gap-2 px-4 h-9 border-b border-subtle/60 shrink-0 min-w-0"
        >
          <span class="text-tertiary font-mono text-[11px] tabular-nums shrink-0">#{{ card.id }}</span>
          <span class="text-tertiary/50 shrink-0" aria-hidden="true">·</span>
          <div class="min-w-0 flex-1">
            <CustomDropdown
              :value="workflowDropdownValue"
              :options="workflowDropdownOptions"
              type="status"
              compact
              inline
              :placeholder="$t('views-ticket-preview-status')"
              @update:value="updateStatus"
            />
          </div>
          <button
            type="button"
            class="inline-flex items-center gap-1 text-[11px] text-secondary hover:text-primary px-2 h-7 rounded-md hover:bg-surface-hover transition-colors"
            @click="onOpen"
          >
            {{ $t('views-ticket-preview-open') }}
            <Icon name="chevronRight" class="w-3 h-3" />
          </button>
          <button
            type="button"
            class="text-tertiary hover:text-primary p-1 rounded hover:bg-surface-hover transition-colors"
            :title="$t('views-ticket-preview-close-tooltip')"
            :aria-label="$t('views-ticket-preview-close-aria')"
            @click="emit('close')"
          >
            <Icon name="close" class="w-3.5 h-3.5" />
          </button>
        </header>

        <div class="flex-1 overflow-y-auto">
          <!-- Title + signal pills -->
          <div class="px-4 pt-3 pb-2.5">
            <textarea
              v-model="titleDraft"
              rows="2"
              class="w-full text-lg font-semibold text-primary leading-snug bg-transparent border-0 outline-none resize-none rounded px-0.5 -mx-0.5 focus:ring-1 focus:ring-accent/30 placeholder:text-tertiary"
              :placeholder="$t('views-ticket-preview-title-placeholder')"
              @blur="commitTitle"
              @keydown.enter.prevent="($event.target as HTMLTextAreaElement).blur()"
            />

            <div class="flex items-center flex-wrap gap-1.5 mt-2.5">
              <span
                v-if="priorityForBadge(card.priority) || card.priority === 'none' || card.priority === 'urgent'"
                class="inline-flex items-center rounded-md border border-subtle h-6 max-w-full"
                :class="priorityToneClass(card.priority)"
              >
                <CustomDropdown
                  :value="selectedPriority"
                  :options="priorityOptions"
                  type="priority"
                  compact
                  inline
                  :placeholder="$t('views-ticket-preview-priority')"
                  @update:value="updatePriority"
                />
              </span>
              <span
                v-if="slaState"
                class="inline-flex items-center gap-1 text-[10px] font-medium px-1.5 h-6 rounded-md border border-subtle transition-colors duration-200"
                :class="slaState.toneClass"
              >
                <Icon name="clock" class="w-3 h-3" />
                {{ slaState.statusLabel }}
              </span>
              <span
                v-if="card.kb_gap_signal && card.kb_gap_signal !== 'none'"
                class="inline-flex items-center gap-1 text-[10px] font-semibold uppercase tracking-wide px-1.5 h-6 rounded-md"
                :class="card.kb_gap_signal === 'strong'
                  ? 'bg-amber-500/15 text-amber-700 dark:text-amber-300'
                  : 'bg-surface-hover text-secondary'"
              >
                <Icon name="warning" class="w-3 h-3" />
                {{ $t('views-ticket-preview-kb-gap') }}
              </span>
              <span
                v-if="card.recurrence_rule"
                class="inline-flex items-center gap-1 text-[10px] font-semibold uppercase tracking-wide px-1.5 h-6 rounded-md bg-violet-500/12 text-violet-700 dark:text-violet-300"
                :title="card.recurrence_rule"
              >
                <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" class="w-3 h-3">
                  <path d="M3 8a5 5 0 0 1 8.5-3.5L13 6M13 6V3M13 6h-3" stroke-linecap="round" stroke-linejoin="round" />
                  <path d="M13 8a5 5 0 0 1-8.5 3.5L3 10M3 10v3M3 10h3" stroke-linecap="round" stroke-linejoin="round" />
                </svg>
                {{ $t('views-ticket-preview-recurring') }}
              </span>
            </div>
          </div>

          <!-- PROPERTIES -->
          <section class="px-4 pt-3 pb-3 border-t border-subtle/60">
            <h3 class="text-[10px] uppercase tracking-wider font-semibold text-tertiary mb-2">
              {{ $t('views-ticket-preview-properties') }}
            </h3>
            <div class="flex flex-col gap-2 text-xs">
              <div class="preview-row flex items-center gap-2 min-h-7">
                <span class="preview-label flex items-center gap-1.5 w-22 shrink-0 text-tertiary">
                  <Icon name="user" class="w-3 h-3" />
                  {{ $t('views-ticket-preview-assignee') }}
                </span>
                <div class="preview-value preview-field-picker flex-1 min-w-0">
                  <UserPicker
                    :model-value="card.assignee_uuid ?? ''"
                    :current-user="assigneeUser"
                    type="assignee"
                    compact
                    :placeholder="$t('views-ticket-preview-not-set')"
                    hide-inline-clear
                    @update:model-value="updateAssignee"
                  />
                </div>
              </div>
              <div class="preview-row flex items-center gap-2 min-h-7">
                <span class="preview-label flex items-center gap-1.5 w-22 shrink-0 text-tertiary">
                  <Icon name="userPlus" class="w-3 h-3" />
                  {{ $t('views-ticket-preview-requester') }}
                </span>
                <UserCell :uuid="card.requester_uuid" class="preview-value flex-1 min-w-0" />
              </div>
              <div class="preview-row flex items-center gap-2 min-h-7">
                <span class="preview-label flex items-center gap-1.5 w-22 shrink-0 text-tertiary">
                  <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" class="w-3 h-3">
                    <rect x="2" y="3" width="12" height="11" rx="1.5" />
                    <line x1="2" y1="6.5" x2="14" y2="6.5" />
                    <line x1="5.5" y1="2" x2="5.5" y2="4" stroke-linecap="round" />
                    <line x1="10.5" y1="2" x2="10.5" y2="4" stroke-linecap="round" />
                  </svg>
                  {{ $t('views-ticket-preview-due-date') }}
                </span>
                <div class="preview-value preview-field-picker flex-1 min-w-0">
                  <DatePicker
                    v-model="dueDateValue"
                    size="sm"
                    block
                    :aria-label="$t('views-ticket-preview-due-date')"
                  />
                </div>
              </div>
              <div v-if="card.cycle_id != null" class="preview-row flex items-center gap-2 min-h-7">
                <span class="preview-label flex items-center gap-1.5 w-22 shrink-0 text-tertiary">
                  <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" class="w-3 h-3">
                    <circle cx="8" cy="8" r="6" stroke-dasharray="3 2" />
                    <circle cx="8" cy="8" r="1.5" fill="currentColor" />
                  </svg>
                  {{ $t('views-ticket-preview-cycle') }}
                </span>
                <span class="preview-value text-accent truncate">{{ $t('views-ticket-preview-cycle-label', { id: card.cycle_id }) }}</span>
              </div>
              <div v-if="card.category_id != null" class="preview-row flex items-center gap-2 min-h-7">
                <span class="preview-label flex items-center gap-1.5 w-22 shrink-0 text-tertiary">
                  <Icon name="tag" class="w-3 h-3" />
                  {{ $t('views-ticket-preview-category') }}
                </span>
                <span class="preview-value text-secondary">#{{ card.category_id }}</span>
              </div>
            </div>
          </section>

          <!-- SLA section: per-timer row(s). When the matched policy
               has both a response and a resolution target the section
               stacks both with labels; with only one configured the
               row is unlabeled (the section heading already names
               what it is). The bar gives peripheral urgency; the
               detail line carries the precise time + target. -->
          <section v-if="slaRows.length > 0" class="px-4 pt-3 pb-3 border-t border-subtle/60 flex flex-col gap-2.5">
            <h3 class="text-[10px] uppercase tracking-wider font-semibold text-tertiary">
              {{ $t('views-ticket-preview-sla') }}
            </h3>
            <div v-for="row in slaRows" :key="row.labelKey ?? 'single'" class="flex flex-col gap-1.5">
              <div class="flex items-center justify-between text-xs">
                <span class="flex items-center gap-2 font-medium transition-colors duration-200" :class="row.state.toneClass">
                  <span v-if="row.labelKey" class="text-tertiary font-normal">{{ $t(row.labelKey) }}</span>
                  {{ row.state.statusLabel }}
                </span>
                <span class="text-tertiary tabular-nums">{{ row.state.target }}</span>
              </div>
              <!-- Time bar. Width animates between cards via the
                   transition on `width` so cross-fading rows feels
                   continuous. -->
              <div class="h-1.5 rounded-full bg-surface-hover overflow-hidden">
                <div
                  class="h-full rounded-full transition-[width,background-color] duration-500"
                  :class="row.state.barClass"
                  :style="{ width: `${row.state.fraction * 100}%` }"
                />
              </div>
              <p class="text-[11px] text-tertiary">{{ row.state.detail }}</p>
            </div>
          </section>

          <!-- ACTIVITY section. Created + last activity stamps so
               the user can read "is this hot or stale?" without
               opening the full route. The two rows use distinct
               glyphs (+ for origin, clock for time-since) so the
               eye can tell them apart at a glance — same Icon
               component on both gives them identical rendered
               size, which inline-SVG mixed with Icon does not. -->
          <section class="px-4 pt-3 pb-3 border-t border-subtle/60">
            <h3 class="text-[10px] uppercase tracking-wider font-semibold text-tertiary mb-2">
              {{ $t('views-ticket-preview-activity') }}
            </h3>
            <div class="flex flex-col gap-2 text-xs">
              <div class="flex items-center gap-3">
                <Icon name="clock" class="w-3.5 h-3.5 text-tertiary shrink-0" />
                <span class="text-secondary flex-1">{{ $t('views-ticket-preview-last-activity') }}</span>
                <span
                  class="text-tertiary tabular-nums"
                  :title="fullDateTime(card.last_activity_at)"
                >{{ relativeOrAbsolute(card.last_activity_at) }}</span>
              </div>
              <div class="flex items-center gap-3">
                <Icon name="circleDot" class="w-3.5 h-3.5 text-tertiary shrink-0" />
                <span class="text-secondary flex-1">{{ $t('views-ticket-preview-created') }}</span>
                <span
                  class="text-tertiary tabular-nums"
                  :title="fullDateTime(card.created_at)"
                >{{ relativeOrAbsolute(card.created_at) }}</span>
              </div>
            </div>
          </section>

          <!-- DEVICES section -->
          <section
            v-if="card.affected_devices && card.affected_devices.count > 0"
            class="px-4 pt-3 pb-3 border-t border-subtle/60"
          >
            <h3 class="text-[10px] uppercase tracking-wider font-semibold text-tertiary mb-2">
              {{ $t('views-ticket-preview-affected-devices') }}
            </h3>
            <div class="text-xs text-secondary flex items-center gap-2">
              <Icon name="device" class="w-3.5 h-3.5 text-tertiary shrink-0" />
              <span v-if="card.affected_devices.first" class="text-primary truncate">
                {{ card.affected_devices.first.name }}
              </span>
              <span
                v-if="card.affected_devices.count > 1"
                class="text-tertiary text-[11px] tabular-nums shrink-0"
              >{{ $t('views-ticket-preview-more-devices', { count: card.affected_devices.count - 1 }) }}</span>
            </div>
          </section>
        </div>
      </div>
    </Transition>
  </aside>
</template>

<style scoped>
/* Cross-fade when arrowing between rows. The leave-active /
   enter-active overlap is what makes this feel like a panel
   "morphing" rather than empty-then-filling. mode=out-in keeps
   us from stacking two ticket bodies on top of each other
   during the transition window. */
.preview-fade-enter-active,
.preview-fade-leave-active {
  transition:
    opacity 140ms cubic-bezier(0.16, 1, 0.3, 1),
    transform 140ms cubic-bezier(0.16, 1, 0.3, 1);
}
.preview-fade-enter-from {
  opacity: 0;
  transform: translateY(4px);
}
.preview-fade-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

@media (prefers-reduced-motion: reduce) {
  .preview-fade-enter-active,
  .preview-fade-leave-active {
    transition: opacity 100ms linear;
  }
  .preview-fade-enter-from,
  .preview-fade-leave-to {
    transform: none;
  }
}

/* Dense property-row controls: match the original read-only row
   height (~28px) so editable fields don't balloon the panel. */
.preview-field-picker :deep(.date-picker__input--sm) {
  min-height: 28px;
  padding-top: 0.25rem;
  padding-bottom: 0.25rem;
}
</style>
