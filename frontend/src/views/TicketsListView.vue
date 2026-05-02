<script setup lang="ts">
import { computed, onMounted, ref, useTemplateRef } from 'vue'
import { useRouter } from 'vue-router'
import { useMutation, useQueryCache } from '@pinia/colada'

import DataTable from '@/components/common/DataTable.vue'
import PaginationControls from '@/components/common/PaginationControls.vue'
import BulkConfirmDialog from '@/components/common/BulkConfirmDialog.vue'
import ListPageLayout, { type ListPageLayoutExpose } from '@/components/common/ListPageLayout.vue'
import FilterRow from '@/components/common/FilterRow.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import { IdCell, TextCell, UserAvatarCell, DateCell } from '@/components/common/cells'
import Modal from '@/components/Modal.vue'
import UserSelectionModal from '@/components/UserSelectionModal.vue'
import StatusBadge from '@/components/StatusBadge.vue'
import UserAvatar from '@/components/UserAvatar.vue'

import { useListControls } from '@/composables/useListControls'
import { useListPage } from '@/composables/useListPage'
import { useBulkSelection } from '@/composables/useBulkSelection'
import { useBulkSelectionForDataTable } from '@/composables/useBulkSelectionForDataTable'
import { useMobileDetection } from '@/composables/useMobileDetection'
import { usePageCreateAction } from '@/composables/usePageCreateAction'
import { useTicketsListLoader } from '@/loaders/ticketsListLoader'
import { useThemeStore } from '@/stores/theme'
import { useAuthStore } from '@/stores/auth'
import { useCollabSessionStore } from '@/stores/collabSession'
import { useWorkflowStatesStore } from '@/stores/workflowStates'
import { parseDate } from '@/utils/dateUtils'
import { STATUS_OPTIONS, PRIORITY_OPTIONS } from '@/constants/ticketOptions'
import { categoryService } from '@/services/categoryService'
import ticketService from '@/services/ticketService'
import { ticketsKeys, TICKETS_FILTER_PARAM_KEYS } from '@/queries/tickets'
import type { TicketCategory } from '@/types/category'
import type { Ticket } from '@/services/ticketService'

defineOptions({ name: 'TicketsListView' })

const router = useRouter()
const themeStore = useThemeStore()
const authStore = useAuthStore()
const queryCache = useQueryCache()
const collab = useCollabSessionStore()
const workflowStatesStore = useWorkflowStatesStore()

// Tighter mobile breakpoint than the default — desktop columns
// for tickets need >=lg to fit comfortably.
const { isMobile } = useMobileDetection('lg')

const layoutRef = useTemplateRef<ListPageLayoutExpose>('layout')
const scrollContainerRef = computed<HTMLElement | null>(
  () => layoutRef.value?.scrollContainerRef ?? null,
)

// Subscribe to the route Data Loader. The loader runs DURING
// navigation and primes the matching infinite-query cache entry,
// so the queries below resolve from cache without firing fresh
// requests on first paint.
useTicketsListLoader()

// Categories load lazily, used to build the category filter
// dropdown; failure leaves the dropdown empty without blocking.
const categories = ref<TicketCategory[]>([])
onMounted(async () => {
  try {
    categories.value = await categoryService.getCategories()
  } catch (err) {
    console.error('Failed to load categories:', err)
  }
})

// Page-size preference: persist user selection to localStorage so
// "All" / 25 / 50 / 100 sticks across sessions.
const PAGESIZE_STORAGE_KEY = 'tickets-page-size'
const savedPageSize = (() => {
  if (typeof localStorage === 'undefined') return 0
  const raw = localStorage.getItem(PAGESIZE_STORAGE_KEY)
  if (raw === null) return 0
  const parsed = parseInt(raw)
  return Number.isFinite(parsed) ? parsed : 0
})()

const navigateToTicket = (ticket: Ticket) => {
  void router.push(`/tickets/${ticket.id}`)
}

/**
 * Hover-prefetch the ticket's collaborative session: opens the
 * websocket and IndexedDB load before the click. The session
 * disconnects on its own after the grace window if the user
 * hovers but doesn't navigate. No-op if the session is already
 * active or warm.
 */
const prewarmTicket = (ticket: Ticket) => {
  collab.warm(`ticket-${ticket.id}`)
}

const handleGoToItem = (itemId: number) => {
  void router.push(`/tickets/${itemId}`)
}

const handleCreateTicket = async () => {
  try {
    const newTicket = await ticketService.createEmptyTicket()
    void router.push(`/tickets/${newTicket.id}`)
  } catch (err) {
    console.error('Failed to create empty ticket:', err)
  }
}

const controls = useListControls<Ticket>({
  itemIdField: 'id',
  defaultSortField: 'id',
  defaultSortDirection: 'desc',
  defaultPageSize: savedPageSize,
})

const page = useListPage({
  controls,
  keys: ticketsKeys,
  fetchPage: (params) =>
    ticketService.getPaginatedTickets(params, `tickets-page-${params.page}`),
  scrollContainerRef,
  sseEvents: ['ticket-updated', 'ticket-created', 'ticket-deleted'],
  mobileSearch: {
    placeholder: 'Search tickets...',
    createIcon: 'ticket',
    onCreate: handleCreateTicket,
  },
  // URL sync covers the full tickets filter set; `status` is the
  // sole multi-select. `current` is a sentinel the filter pickers
  // emit for assignee/requester, the loader resolves it to the
  // logged-in user's uuid before the request is built.
  urlSync: {
    paramKeys: TICKETS_FILTER_PARAM_KEYS,
    multiSelectKeys: ['status'],
    transformValue: (key, value) => {
      if ((key === 'assignee' || key === 'requester') && value === 'current') {
        return authStore.user?.uuid ?? value
      }
      return value
    },
  },
})

usePageCreateAction(handleCreateTicket)

const selection = useBulkSelection<Ticket>({
  items: page.items,
  cacheKey: controls.cacheKeyPart,
  totalCount: page.totalItems,
})
const dt = useBulkSelectionForDataTable(selection)

// Persist page size on every change.
const handlePageSizeChange = (size: number) => {
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem(PAGESIZE_STORAGE_KEY, String(size))
  }
  controls.handlePageSizeChange(size)
}

// Compact date/time format for mobile.
const formatCompactDateTime = (dateString: string): string => {
  const date = parseDate(dateString)
  if (!date) return ''
  const now = new Date()
  const isToday = date.toDateString() === now.toDateString()
  const isThisYear = date.getFullYear() === now.getFullYear()
  if (isToday) {
    return date.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' })
  }
  if (isThisYear) {
    return (
      date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' }) +
      ' ' +
      date.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' })
    )
  }
  return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: '2-digit' })
}

const columns = [
  { field: 'id', label: 'ID', width: 'minmax(60px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'title', label: 'Title', width: '1fr', sortable: true, responsive: 'always' as const },
  { field: 'status', label: 'Status', width: 'minmax(85px,auto)', sortable: true, responsive: 'always' as const },
  { field: 'priority', label: 'Priority', width: 'minmax(75px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'created', label: 'Created', width: 'minmax(90px,auto)', sortable: true, sortKey: 'created_at', responsive: 'lg' as const },
  { field: 'requester', label: 'Requester', width: 'minmax(120px,auto)', sortable: true, sortKey: 'requester_uuid', responsive: 'lg' as const },
  { field: 'assignee', label: 'Assignee', width: 'minmax(120px,auto)', sortable: true, sortKey: 'assignee_uuid', responsive: 'lg' as const },
]

const filterOptions = computed(() => {
  const categoryOptions = categories.value.map((cat) => ({
    value: String(cat.id),
    label: cat.name,
  }))

  return controls.buildFilterOptions({
    status: {
      options: STATUS_OPTIONS,
      width: 'w-[130px]',
      allLabel: 'All Statuses',
      placeholder: 'Status',
      multiple: true,
    },
    priority: {
      options: PRIORITY_OPTIONS,
      width: 'w-[130px]',
      allLabel: 'All Priorities',
      placeholder: 'Priority',
    },
    category: {
      options: categoryOptions,
      width: 'w-[140px]',
      allLabel: 'All Categories',
      placeholder: 'Category',
    },
  })
})

// Bulk action modals: status, priority, assign (domain-specific
// pickers, not generic confirms). Delete uses BulkConfirmDialog.
const showStatusModal = ref(false)
const showPriorityModal = ref(false)
const showAssignModal = ref(false)
const showDeleteConfirm = ref(false)

const bulkActionMutation = useMutation({
  mutation: (vars: {
    action: 'delete' | 'set-status' | 'set-priority' | 'assign'
    ids: number[]
    value?: string
  }) => ticketService.bulkAction(vars),
  // Server-authoritative for bulk operations: re-fetch instead of
  // optimistically reconciling a multi-row change across infinite
  // + paginated cache entries.
  onSettled: () => queryCache.invalidateQueries({ key: ticketsKeys.root }),
  onError: (err) => {
    console.error('Bulk action failed:', err)
    alert('Failed to perform bulk action. Please try again.')
  },
})

async function executeBulk(
  action: 'delete' | 'set-status' | 'set-priority' | 'assign',
  value?: string,
) {
  const ids = selection.selectedIds.value.map((id) => parseInt(id))
  if (ids.length === 0) return
  await bulkActionMutation.mutateAsync({ action, ids, value })
  selection.clear()
  showStatusModal.value = false
  showPriorityModal.value = false
  showAssignModal.value = false
  showDeleteConfirm.value = false
}
</script>

<template>
  <!-- Single root div, see App.vue's Transition note. -->
  <div class="h-full">
    <ListPageLayout
      ref="layout"
      :items="page.items.value"
      :total-items="page.totalItems.value"
      :is-first-load="page.isFirstLoad.value"
      :is-background-refresh="page.isBackgroundRefresh.value"
      :is-loading-more="page.isLoadingMore.value"
      :error="page.errorMessage.value"
      :search-query="controls.searchQuery.value"
      search-placeholder="Search tickets..."
      item-label="ticket"
      :bulk-selection="selection"
      @update:search-query="controls.handleSearchUpdate"
      @retry="page.handleRetry"
    >
      <template #filters>
        <FilterRow
          :options="filterOptions"
          @update="controls.handleFilterUpdate"
          @reset="controls.resetFilters"
        />
      </template>

      <!-- Don't show the "X results" badge in infinite mode (the
           total isn't a meaningful position indicator there). -->
      <template v-if="!controls.isInfiniteMode.value" #search-meta>
        {{ page.totalItems.value }} result{{ page.totalItems.value !== 1 ? 's' : '' }}
      </template>

      <template #empty-state>
        <EmptyState
          icon="ticket"
          :title="controls.searchQuery.value ? 'No tickets match your search' : 'No tickets found'"
          :description="controls.searchQuery.value ? 'Try adjusting your search or filters' : 'Create your first ticket to get started'"
        />
      </template>

      <template #desktop="{ items, isBackgroundRefresh }">
        <DataTable
          :columns="columns"
          :data="items"
          :selected-items="dt.selectedItems"
          :sort-field="controls.sortField.value"
          :sort-direction="controls.sortDirection.value"
          :loading="isBackgroundRefresh"
          @update:sort="controls.handleSortUpdate"
          @toggle-selection="dt.onToggleSelection"
          @toggle-all="dt.onToggleAll"
          @row-click="navigateToTicket"
          @row-mouseenter="prewarmTicket"
        >
          <template #cell-id="{ item }">
            <IdCell :id="item.id" />
          </template>
          <template #cell-title="{ item }">
            <TextCell :value="item.title" font-weight="medium" />
          </template>
          <template #cell-status="{ item }">
            <StatusBadge
              type="status"
              :value="item.status"
              :workflow-state="item.workflow_state_id ? workflowStatesStore.findById(item.workflow_state_id) : null"
              :short="true"
              :compact="true"
            />
          </template>
          <template #cell-priority="{ item }">
            <StatusBadge type="priority" :value="item.priority" :short="true" :compact="true" />
          </template>
          <template #cell-created="{ item }">
            <DateCell :value="item.created" format="compact" />
          </template>
          <template #cell-requester="{ item }">
            <UserAvatarCell
              :user-id="item.requester_user?.uuid || item.requester"
              :avatar="item.requester_user?.avatar_thumb"
              :user-name="item.requester_user?.name || item.requester"
              :show-name="true"
            />
          </template>
          <template #cell-assignee="{ item }">
            <UserAvatarCell
              :user-id="item.assignee_user?.uuid || item.assignee"
              :avatar="item.assignee_user?.avatar_thumb"
              :user-name="item.assignee_user?.name || item.assignee"
              :show-name="true"
            />
          </template>
        </DataTable>
      </template>

      <template #mobile-row="{ item }">
        <div
          class="flex items-center gap-3 px-3 py-2.5 hover:bg-surface-hover active:bg-surface-alt transition-colors cursor-pointer border-t border-default first:border-t-0"
          v-memo="[item.id, item.title, item.status, item.priority, item.created, item.requester, item.assignee, themeStore.effectiveColorBlindMode]"
          @click="navigateToTicket(item)"
          @mouseenter="prewarmTicket(item)"
        >
          <div
            v-if="themeStore.effectiveColorBlindMode"
            class="w-2 self-stretch rounded-full flex-shrink-0 relative box-border"
            :class="{
              'border-2 border-status-open bg-transparent': item.status === 'open',
              'border-2 border-status-in-progress bg-transparent': item.status === 'in-progress',
              'bg-status-closed': item.status === 'closed',
            }"
          >
            <div
              v-if="item.status === 'in-progress'"
              class="absolute inset-x-0 bottom-0 h-1/2 bg-status-in-progress rounded-b-full"
              style="left: -2px; right: -2px; bottom: -2px;"
            ></div>
          </div>
          <div
            v-else
            class="w-1.5 self-stretch rounded-full flex-shrink-0"
            :class="{
              'bg-status-open': item.status === 'open',
              'bg-status-in-progress': item.status === 'in-progress',
              'bg-status-closed': item.status === 'closed',
            }"
          ></div>

          <div class="flex-1 min-w-0">
            <div class="flex items-center gap-2">
              <span class="text-xs text-secondary font-medium flex-shrink-0">#{{ item.id }}</span>
              <span class="text-sm text-primary font-medium truncate">{{ item.title }}</span>
            </div>

            <div class="flex flex-wrap items-center gap-x-3 gap-y-1 mt-1.5 text-xs">
              <div class="flex items-center gap-2 flex-shrink-0">
                <StatusBadge
                  type="status"
                  :value="item.status"
                  :workflow-state="item.workflow_state_id ? workflowStatesStore.findById(item.workflow_state_id) : null"
                  :short="true"
                  :compact="true"
                />
                <StatusBadge type="priority" :value="item.priority" :short="true" :compact="true" />
              </div>

              <span class="text-tertiary flex-shrink-0">{{ formatCompactDateTime(item.created) }}</span>

              <div class="flex items-center gap-1 min-w-0">
                <span class="text-tertiary flex-shrink-0">From:</span>
                <div class="flex items-center gap-1 min-w-0">
                  <div class="flex-shrink-0 [&>div]:!w-4 [&>div]:!h-4 [&>div>*]:!w-4 [&>div>*]:!h-4 [&>div>*]:!text-[0.5rem]">
                    <UserAvatar
                      v-if="item.requester_user?.uuid || item.requester"
                      :name="item.requester_user?.uuid || item.requester"
                      :user-name="item.requester_user?.name"
                      :avatar="item.requester_user?.avatar_thumb"
                      size="xs"
                      :show-name="false"
                      :clickable="false"
                    />
                  </div>
                  <span class="text-secondary truncate max-w-[120px]">{{ item.requester_user?.name || item.requester || 'Unknown' }}</span>
                </div>
              </div>

              <div class="flex items-center gap-1 min-w-0">
                <span class="text-tertiary flex-shrink-0">To:</span>
                <div class="flex items-center gap-1 min-w-0">
                  <template v-if="item.assignee_user?.name || item.assignee">
                    <div class="flex-shrink-0 [&>div]:!w-4 [&>div]:!h-4 [&>div>*]:!w-4 [&>div>*]:!h-4 [&>div>*]:!text-[0.5rem]">
                      <UserAvatar
                        :name="item.assignee_user?.uuid || item.assignee"
                        :user-name="item.assignee_user?.name"
                        :avatar="item.assignee_user?.avatar_thumb"
                        size="xs"
                        :show-name="false"
                        :clickable="false"
                      />
                    </div>
                    <span class="text-secondary truncate max-w-[120px]">{{ item.assignee_user?.name || item.assignee }}</span>
                  </template>
                  <span v-else class="text-tertiary italic">Unassigned</span>
                </div>
              </div>
            </div>
          </div>

          <svg class="w-4 h-4 text-tertiary flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
          </svg>
        </div>
      </template>

      <template #bulk-actions="{ selectedCount }">
        <button
          type="button"
          class="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full text-secondary hover:text-primary hover:bg-surface-hover transition-colors whitespace-nowrap disabled:opacity-50"
          :disabled="bulkActionMutation.asyncStatus.value === 'loading'"
          @click="showStatusModal = true"
        >
          Status
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full text-secondary hover:text-primary hover:bg-surface-hover transition-colors whitespace-nowrap disabled:opacity-50"
          :disabled="bulkActionMutation.asyncStatus.value === 'loading'"
          @click="showPriorityModal = true"
        >
          Priority
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full text-secondary hover:text-primary hover:bg-surface-hover transition-colors whitespace-nowrap disabled:opacity-50"
          :disabled="bulkActionMutation.asyncStatus.value === 'loading'"
          @click="showAssignModal = true"
        >
          Assign
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full text-status-error hover:bg-status-error/10 transition-colors whitespace-nowrap disabled:opacity-50"
          :disabled="bulkActionMutation.asyncStatus.value === 'loading'"
          @click="showDeleteConfirm = true"
        >
          Delete{{ selectedCount > 0 ? ` ${selectedCount}` : '' }}
        </button>
      </template>

      <template #footer>
        <PaginationControls
          v-if="!isMobile || !controls.isInfiniteMode.value"
          :current-page="controls.currentPage.value"
          :total-pages="page.totalPages.value"
          :total-items="page.totalItems.value"
          :page-size="controls.pageSize.value"
          :page-size-options="controls.pageSizeOptions"
          :is-infinite-mode="controls.isInfiniteMode.value"
          @update:current-page="controls.handlePageChange"
          @update:page-size="handlePageSizeChange"
          @go-to-item="handleGoToItem"
        />
      </template>
    </ListPageLayout>

    <BulkConfirmDialog
      :show="showDeleteConfirm"
      :count="selection.selectedCount.value"
      item-label="ticket"
      action-verb="delete"
      @confirm="executeBulk('delete')"
      @close="showDeleteConfirm = false"
    />

    <Modal :show="showStatusModal" title="Set Status" size="sm" @close="showStatusModal = false">
      <div class="flex flex-col gap-2 p-4">
        <p class="text-sm text-secondary mb-2">
          Update status for {{ selection.selectedCount.value }} ticket{{ selection.selectedCount.value !== 1 ? 's' : '' }}
        </p>
        <button
          v-for="status in STATUS_OPTIONS"
          :key="status.value"
          :disabled="bulkActionMutation.asyncStatus.value === 'loading'"
          class="flex items-center gap-3 px-4 py-3 rounded-lg hover:bg-surface-hover transition-colors text-left disabled:opacity-50"
          @click="executeBulk('set-status', status.value)"
        >
          <StatusBadge type="status" :value="status.value" />
          <span class="text-primary">{{ status.label }}</span>
        </button>
      </div>
    </Modal>

    <Modal :show="showPriorityModal" title="Set Priority" size="sm" @close="showPriorityModal = false">
      <div class="flex flex-col gap-2 p-4">
        <p class="text-sm text-secondary mb-2">
          Update priority for {{ selection.selectedCount.value }} ticket{{ selection.selectedCount.value !== 1 ? 's' : '' }}
        </p>
        <button
          v-for="priority in PRIORITY_OPTIONS"
          :key="priority.value"
          :disabled="bulkActionMutation.asyncStatus.value === 'loading'"
          class="flex items-center gap-3 px-4 py-3 rounded-lg hover:bg-surface-hover transition-colors text-left disabled:opacity-50"
          @click="executeBulk('set-priority', priority.value)"
        >
          <StatusBadge type="priority" :value="priority.value" />
          <span class="text-primary">{{ priority.label }}</span>
        </button>
      </div>
    </Modal>

    <UserSelectionModal
      :show="showAssignModal"
      title="Assign Tickets"
      @close="showAssignModal = false"
      @select="executeBulk('assign', $event)"
    />
  </div>
</template>
