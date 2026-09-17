<script setup lang="ts">
/**
 * Floating bulk-action bar for the tickets list, on the shared
 * `BulkActionBar` (a Reka Toolbar: one tab stop, arrows walk the
 * buttons, the count / select-all / clear chrome is the bar's). This
 * component owns the ticket actions Linear / Asana / GitHub Issues
 * all surface for triage: Status, Priority, Assignee, plus Merge and
 * any plugin actions.
 *
 * Status / Priority are Reka Listboxes in a popover (small option
 * sets, no search needed). Assignee opens the existing
 * UserSelectionModal, search-driven because the user list can grow
 * large.
 *
 * The bar is presentational: dispatching the actual mutations is the
 * parent's job. We emit the chosen value + the selected ids and let
 * TicketsListView wire it through the sync engine.
 */
import { computed, nextTick, ref, watch, type Ref } from 'vue'
import { ListboxContent, ListboxGroup, ListboxGroupLabel, ListboxItem, ListboxRoot, ToolbarButton } from 'reka-ui'
import BulkActionBar from '@/components/common/BulkActionBar.vue'
import Popover from '@/components/common/Popover.vue'
import Icon from '@/components/common/Icon.vue'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import WorkflowStateGlyph from '@/components/views/WorkflowStateGlyph.vue'
import UserSelectionModal from '@/components/UserSelectionModal.vue'
import MergeTicketsDialog, {
  type MergeDialogTicket,
} from '@/components/ticketComponents/MergeTicketsDialog.vue'
import type { PopoverAnchor } from '@/composables/popoverAnchor'
import { useWorkflowStatesStore } from '@nosdesk/core/stores/workflowStates'
import { PRIORITY_OPTIONS } from '@nosdesk/core/constants/ticketOptions'
import type { TicketPriority } from '@nosdesk/core/constants/ticketOptions'
import { WORKFLOW_CATEGORIES, getCategoryLabel, type WorkflowStateCategory } from '@nosdesk/core/types/workflow'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import { getSlotRegistrations } from '@/plugins/loader'
import { openPluginModal } from '@/plugins/usePluginModal'

const props = defineProps<{
  /** Selected ticket ids (strings, since useBulkSelection's set
   *  is opaque to the value type — the parent stringifies the
   *  numeric ids before passing them in). */
  selectedIds: string[]
  /** Total ticket count matching the current view; drives the
   *  "Select all N" affordance. Optional. */
  totalCount?: number
}>()

const emit = defineEmits<{
  (e: 'clear'): void
  (e: 'select-all'): void
  /** Action chosen + the ticket ids it should be applied to.
   *  Both pieces emitted so the parent has everything it needs
   *  in one shot — no `selectedIds` lookup race in the handler. */
  (e: 'set-status', stateId: number, ticketIds: number[]): void
  (e: 'set-priority', priority: string, ticketIds: number[]): void
  (e: 'set-assignee', assigneeUuid: string, ticketIds: number[]): void
}>()

const workflowStatesStore = useWorkflowStatesStore()
const ticketsStore = useSyncTicketsStore()

// Numeric ids — useBulkSelection works in strings (opaque) but
// the mutation layer takes numbers. Casting once here keeps the
// handlers simple.
const ids = computed<number[]>(() =>
  props.selectedIds.map((s) => Number(s)).filter((n) => Number.isFinite(n)),
)

const selectedCount = computed<number>(() => ids.value.length)
// Sample the first selected ticket's current values so the
// popover can highlight "current" when every selected ticket
// shares the same state / priority. Tiny UX touch — when 5
// tickets are all "Open", the picker shows "Open" as the
// current selection rather than nothing.
const sharedWorkflowStateId = computed<number | null>(() => {
  if (ids.value.length === 0) return null
  const first = ticketsStore.byId(ids.value[0]).value
  if (!first) return null
  const stateId = first.workflow_state_id
  for (const id of ids.value.slice(1)) {
    const t = ticketsStore.byId(id).value
    if (!t || t.workflow_state_id !== stateId) return null
  }
  return stateId
})
const sharedPriority = computed<string | null>(() => {
  if (ids.value.length === 0) return null
  const first = ticketsStore.byId(ids.value[0]).value
  if (!first) return null
  const p = first.priority
  for (const id of ids.value.slice(1)) {
    const t = ticketsStore.byId(id).value
    if (!t || t.priority !== p) return null
  }
  return p
})

const sharedWorkflowState = computed(() => {
  const id = sharedWorkflowStateId.value
  if (id == null) return null
  return workflowStatesStore.findById(id) ?? null
})

function isTicketPriority(value: string | null): value is TicketPriority {
  return value === 'low' || value === 'medium' || value === 'high'
}

// Status options grouped by workflow category. Each row carries
// the category so WorkflowStateGlyph can encode state as shape +
// hue (colour-blind friendly), matching the tickets table.
const statusGroups = computed(() => {
  const out: {
    label: string
    states: {
      id: number
      name: string
      color: string
      category: WorkflowStateCategory
    }[]
  }[] = []
  const grouped = workflowStatesStore.byCategory
  for (const cat of WORKFLOW_CATEGORIES) {
    const states = grouped[cat]
    if (!states || states.length === 0) continue
    out.push({
      label: getCategoryLabel(cat),
      states: states.map((s) => ({
        id: s.id,
        name: s.name,
        color: s.color,
        category: s.category,
      })),
    })
  }
  return out
})

// ---- Popover plumbing ------------------------------------------
const statusBtnRef = ref<HTMLElement | null>(null)
const priorityBtnRef = ref<HTMLElement | null>(null)
const statusOpen = ref(false)
const priorityOpen = ref(false)
const showAssignModal = ref(false)

const statusAnchor = computed<PopoverAnchor>(() => ({
  type: 'element' as const,
  element: () => statusBtnRef.value,
}))
const priorityAnchor = computed<PopoverAnchor>(() => ({
  type: 'element' as const,
  element: () => priorityBtnRef.value,
}))

// Focus lands on the shared value (or the first option) when a list
// opens: Reka highlights it on mount without focusing, and the
// popover's own focus scope would take the first option, so it is off.
type ListboxHandle = { highlightSelected: () => Promise<void> }
const statusList = ref<ListboxHandle | null>(null)
const priorityList = ref<ListboxHandle | null>(null)
function focusOnOpen(open: Ref<boolean>, list: Ref<ListboxHandle | null>) {
  watch(open, async (isOpen) => {
    if (!isOpen) return
    await nextTick()
    await list.value?.highlightSelected()
  })
}
focusOnOpen(statusOpen, statusList)
focusOnOpen(priorityOpen, priorityList)

function pickStatus(value: unknown): void {
  const stateId = Number(value)
  if (!Number.isFinite(stateId)) return
  statusOpen.value = false
  emit('set-status', stateId, ids.value)
}
function pickPriority(value: unknown): void {
  if (typeof value !== 'string') return
  priorityOpen.value = false
  emit('set-priority', value, ids.value)
}
function onAssignSelect(user: { uuid: string }): void {
  showAssignModal.value = false
  emit('set-assignee', user.uuid, ids.value)
}

// ---- Merge ----------------------------------------------------
const showMergeDialog = ref(false)
// Resolve the selection to the minimal shape the merge dialog needs.
// (The sync store's SyncTicket carries id / title / workflow_state_id;
// merged tickets are filtered out of the list, and the backend rejects
// an already-merged source, so the count check is enough here.)
const selectedTickets = computed<MergeDialogTicket[]>(() =>
  ids.value
    .map((id) => ticketsStore.byId(id).value)
    .filter((t): t is NonNullable<typeof t> => !!t)
    .map((t) => ({ id: t.id, title: t.title, workflow_state_id: t.workflow_state_id })),
)
const canMerge = computed(() => selectedTickets.value.length >= 2)
function onMerged(): void {
  showMergeDialog.value = false
  emit('clear')
}

// ---- Plugin bulk actions --------------------------------------
// Plugins contributing a `ticket.bulk.action` render as buttons here; clicking
// one opens the plugin's component in the on-demand modal, handing it the
// current ticket-id selection. Labels are already i18n-resolved at load time.
const pluginBulkActions = computed(() => getSlotRegistrations('ticket.bulk.action'))
function runPluginBulkAction(reg: { pluginUuid: string; componentName: string }): void {
  openPluginModal({
    pluginUuid: reg.pluginUuid,
    componentName: reg.componentName,
    slot: 'ticket.bulk.action',
    context: { ticketIds: ids.value },
  })
}
</script>

<template>
  <BulkActionBar
    :selected-count="selectedCount"
    :total-count="totalCount ?? 0"
    selection-copy-key="bulk-bar-tickets-selected"
    all-selected-copy-key="bulk-bar-tickets-all-selected"
    @select-all-matching="emit('select-all')"
    @clear="emit('clear')"
  >
    <template #actions>
      <!-- Status -->
      <ToolbarButton as-child>
        <button
          ref="statusBtnRef"
          type="button"
          class="inline-flex items-center gap-1 px-2 py-1 rounded-full text-xs text-secondary hover:text-primary hover:bg-surface-hover transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          aria-haspopup="dialog"
          :aria-expanded="statusOpen"
          @click="statusOpen = !statusOpen"
        >
          <WorkflowStateGlyph
            v-if="sharedWorkflowState"
            :category="sharedWorkflowState.category"
            :color="sharedWorkflowState.color"
            :name="sharedWorkflowState.name"
            :size="14"
          />
          <Icon v-else name="circleDot" class="w-3.5 h-3.5" />
          <span>{{ $t('ticket-list-bulk-status') }}</span>
          <Icon name="chevronDown" class="w-3 h-3 text-tertiary" />
        </button>
      </ToolbarButton>
      <Popover
        :open="statusOpen"
        :anchor="statusAnchor"
        placement="top-start"
        react-to-scroll="reposition"
        :auto-focus="false"
        role="dialog"
        :aria-label="$t('ticket-list-bulk-status')"
        popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[200px] max-h-[320px] overflow-y-auto"
        @close="statusOpen = false"
      >
        <ListboxRoot
          ref="statusList"
          :model-value="sharedWorkflowStateId ?? undefined"
          selection-behavior="replace"
          highlight-on-hover
          @update:model-value="pickStatus"
        >
          <ListboxContent class="outline-none" :aria-label="$t('ticket-list-bulk-status')">
            <ListboxGroup v-for="group in statusGroups" :key="group.label">
              <ListboxGroupLabel class="px-3 pt-2 pb-1 text-3xs font-semibold text-tertiary tracking-wide uppercase">
                {{ group.label }}
              </ListboxGroupLabel>
              <ListboxItem v-for="state in group.states" :key="state.id" as-child :value="state.id">
                <button
                  type="button"
                  class="w-full flex items-center gap-2 px-3 py-1.5 text-sm text-left text-primary hover:bg-surface-hover data-[highlighted]:bg-surface-hover transition-colors outline-none"
                  :class="{ 'bg-accent/10': sharedWorkflowStateId === state.id }"
                >
                  <WorkflowStateGlyph
                    :category="state.category"
                    :color="state.color"
                    :name="state.name"
                    :size="14"
                  />
                  <span
                    class="flex-1 truncate"
                    :class="{ 'font-medium': sharedWorkflowStateId === state.id }"
                  >{{ state.name }}</span>
                  <Icon
                    v-if="sharedWorkflowStateId === state.id"
                    name="check"
                    class="w-3 h-3 text-accent shrink-0"
                  />
                </button>
              </ListboxItem>
            </ListboxGroup>
          </ListboxContent>
        </ListboxRoot>
      </Popover>

      <!-- Priority -->
      <ToolbarButton as-child>
        <button
          ref="priorityBtnRef"
          type="button"
          class="inline-flex items-center gap-1 px-2 py-1 rounded-full text-xs text-secondary hover:text-primary hover:bg-surface-hover transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          aria-haspopup="dialog"
          :aria-expanded="priorityOpen"
          @click="priorityOpen = !priorityOpen"
        >
          <PriorityIndicator
            v-if="isTicketPriority(sharedPriority)"
            :priority="sharedPriority"
            size="sm"
          />
          <span
            v-else
            class="inline-flex w-3.5 h-3.5 items-center justify-center text-tertiary"
            aria-hidden="true"
          >
            <span class="w-2 h-2 rounded-full border border-current" />
          </span>
          <span>{{ $t('ticket-list-bulk-priority') }}</span>
          <Icon name="chevronDown" class="w-3 h-3 text-tertiary" />
        </button>
      </ToolbarButton>
      <Popover
        :open="priorityOpen"
        :anchor="priorityAnchor"
        placement="top-start"
        react-to-scroll="reposition"
        :auto-focus="false"
        role="dialog"
        :aria-label="$t('ticket-list-bulk-priority')"
        popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[160px]"
        @close="priorityOpen = false"
      >
        <ListboxRoot
          ref="priorityList"
          :model-value="sharedPriority ?? undefined"
          selection-behavior="replace"
          highlight-on-hover
          @update:model-value="pickPriority"
        >
          <ListboxContent class="outline-none" :aria-label="$t('ticket-list-bulk-priority')">
            <ListboxItem v-for="opt in PRIORITY_OPTIONS" :key="opt.value" as-child :value="opt.value">
              <button
                type="button"
                class="w-full flex items-center gap-2 px-3 py-1.5 text-sm text-left text-primary hover:bg-surface-hover data-[highlighted]:bg-surface-hover transition-colors outline-none"
                :class="{ 'bg-accent/10': sharedPriority === opt.value }"
              >
                <PriorityIndicator :priority="opt.value" size="sm" />
                <span
                  class="flex-1"
                  :class="{ 'font-medium': sharedPriority === opt.value }"
                >{{ $t(opt.labelKey) }}</span>
                <Icon
                  v-if="sharedPriority === opt.value"
                  name="check"
                  class="w-3 h-3 text-accent shrink-0"
                />
              </button>
            </ListboxItem>
          </ListboxContent>
        </ListboxRoot>
      </Popover>

      <!-- Assignee: opens the existing modal because the user list
           can be large enough to need search. -->
      <ToolbarButton as-child>
        <button
          type="button"
          class="inline-flex items-center gap-1 px-2 py-1 rounded-full text-xs text-secondary hover:text-primary hover:bg-surface-hover transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          @click="showAssignModal = true"
        >
          <Icon name="user" class="w-3.5 h-3.5" />
          <span>{{ $t('ticket-list-bulk-assign') }}</span>
        </button>
      </ToolbarButton>

      <!-- Merge: only when 2+ tickets are selected. -->
      <ToolbarButton v-if="canMerge" as-child>
        <button
          type="button"
          class="inline-flex items-center gap-1 px-2 py-1 rounded-full text-xs text-secondary hover:text-primary hover:bg-surface-hover transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          @click="showMergeDialog = true"
        >
          <Icon name="link" class="w-3.5 h-3.5" />
          <span>{{ $t('ticket-list-bulk-merge') }}</span>
        </button>
      </ToolbarButton>

      <!-- Plugin bulk actions: each opens the plugin's component in
           a modal with the current selection. -->
      <ToolbarButton
        v-for="action in pluginBulkActions"
        :key="`${action.pluginUuid}:${action.componentName}`"
        as-child
      >
        <button
          type="button"
          class="inline-flex items-center gap-1 px-2 py-1 rounded-full text-xs text-secondary hover:text-primary hover:bg-surface-hover transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          @click="runPluginBulkAction(action)"
        >
          <img v-if="action.icon" :src="action.icon" class="w-3.5 h-3.5" alt="" />
          <Icon v-else name="puzzle" class="w-3.5 h-3.5" />
          <span>{{ action.label ?? action.pluginName }}</span>
        </button>
      </ToolbarButton>
    </template>
  </BulkActionBar>

  <UserSelectionModal
    :show="showAssignModal"
    @close="showAssignModal = false"
    @select-user="onAssignSelect"
  />

  <MergeTicketsDialog
    :open="showMergeDialog"
    :selected-tickets="selectedTickets"
    @close="showMergeDialog = false"
    @merged="onMerged"
  />
</template>
