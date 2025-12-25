// views/TicketsListView.vue
<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import ticketService from "@/services/ticketService";
import BaseListView from "@/components/common/BaseListView.vue";
import DataTable from "@/components/common/DataTable.vue";
import DebouncedSearchInput from "@/components/common/DebouncedSearchInput.vue";
import PaginationControls from "@/components/common/PaginationControls.vue";
import { IdCell, TextCell, UserAvatarCell, DateCell } from "@/components/common/cells";
import BaseDropdown from "@/components/common/BaseDropdown.vue";
import BulkActionsBar from "@/components/common/BulkActionsBar.vue";
import type { BulkAction } from "@/components/common/BulkActionsBar.vue";
import Modal from "@/components/Modal.vue";
import UserSelectionModal from "@/components/UserSelectionModal.vue";
import StatusBadge from "@/components/StatusBadge.vue";
import UserAvatar from "@/components/UserAvatar.vue";
import { useListManagement } from "@/composables/useListManagement";
import { useListSSE } from "@/composables/useListSSE";
import { useStaggeredList } from "@/composables/useStaggeredList";
import { useMobileDetection } from "@/composables/useMobileDetection";
import { useInfiniteScroll } from "@/composables/useInfiniteScroll";
import { useThemeStore } from "@/stores/theme";
import { parseDate } from "@/utils/dateUtils";
import { STATUS_OPTIONS, PRIORITY_OPTIONS } from "@/constants/ticketOptions";
import type { Ticket } from "@/services/ticketService";

const themeStore = useThemeStore();
const route = useRoute();
const router = useRouter();

// Shared mobile detection (lg breakpoint = 1024px)
const { isMobile } = useMobileDetection('lg');

// Refs for scroll containers
const desktopScrollContainer = ref<HTMLElement | null>(null);
const mobileScrollContainer = ref<HTMLElement | null>(null);

// Compact date/time format for mobile
const formatCompactDateTime = (dateString: string): string => {
  const date = parseDate(dateString);
  if (!date) return '';

  const now = new Date();
  const isToday = date.toDateString() === now.toDateString();
  const isThisYear = date.getFullYear() === now.getFullYear();

  if (isToday) {
    return date.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' });
  } else if (isThisYear) {
    return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' }) +
           ' ' + date.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' });
  }
  return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: '2-digit' });
};

// Handler for go-to-item event from PaginationControls
const handleGoToItem = (itemId: number) => {
  router.push(`/tickets/${itemId}`);
};

// Bulk actions configuration
const bulkActions: BulkAction[] = [
  { id: 'set-status', label: 'Status', icon: 'status' },
  { id: 'set-priority', label: 'Priority', icon: 'tag' },
  { id: 'assign', label: 'Assign', icon: 'assign' },
  { id: 'delete', label: 'Delete', icon: 'delete', variant: 'danger', confirm: true }
];

// Bulk action modal states
const showStatusModal = ref(false);
const showPriorityModal = ref(false);
const showAssignModal = ref(false);
const bulkActionLoading = ref(false);

// Handle bulk action selection
const handleBulkAction = async (actionId: string) => {
  if (actionId === 'set-status') {
    showStatusModal.value = true;
  } else if (actionId === 'set-priority') {
    showPriorityModal.value = true;
  } else if (actionId === 'assign') {
    showAssignModal.value = true;
  } else if (actionId === 'delete') {
    await executeBulkAction('delete');
  }
};

// Execute bulk action
const executeBulkAction = async (
  action: 'delete' | 'set-status' | 'set-priority' | 'assign',
  value?: string
) => {
  const ids = listManager.selectedItems.value.map(id => parseInt(id));
  if (ids.length === 0) return;

  bulkActionLoading.value = true;
  try {
    await ticketService.bulkAction({ action, ids, value });

    // Refresh the list and clear selection
    await listManager.refresh();
    listManager.clearSelection();

    // Close any open modals
    showStatusModal.value = false;
    showPriorityModal.value = false;
    showAssignModal.value = false;
  } catch (error) {
    console.error('Bulk action failed:', error);
    alert('Failed to perform bulk action. Please try again.');
  } finally {
    bulkActionLoading.value = false;
  }
};

// Handle bulk status change
const handleBulkStatusChange = (status: string) => {
  executeBulkAction('set-status', status);
};

// Handle bulk priority change
const handleBulkPriorityChange = (priority: string) => {
  executeBulkAction('set-priority', priority);
};

// Handle bulk assign
const handleBulkAssign = (userId: string) => {
  executeBulkAction('assign', userId);
  showAssignModal.value = false;
};

// Extract URL params for initial state
const urlParams = route.query;
const initialFilters: Record<string, string | string[]> = {};

// Define which filters support multiple values
const multiSelectFilters = ['status'];

// Set initial values from URL
const filterKeys = ['status', 'priority', 'createdOn', 'createdAfter', 'createdBefore',
                    'modifiedOn', 'modifiedAfter', 'modifiedBefore', 'closedOn', 'closedAfter', 'closedBefore'];
filterKeys.forEach(key => {
  if (urlParams[key] && typeof urlParams[key] === 'string') {
    const value = urlParams[key] as string;
    // Parse comma-separated values for multi-select filters
    if (multiSelectFilters.includes(key) && value.includes(',')) {
      initialFilters[key] = value.split(',');
    } else if (multiSelectFilters.includes(key)) {
      // Single value for multi-select becomes an array
      initialFilters[key] = [value];
    } else {
      initialFilters[key] = value;
    }
  }
});

const initialSearchQuery = (urlParams.search && typeof urlParams.search === 'string') ? urlParams.search : '';
const initialPage = (urlParams.page && typeof urlParams.page === 'string') ? parseInt(urlParams.page) : 1;

// Page size preference: URL param > localStorage > default (0 = all/infinite)
const PAGESIZE_STORAGE_KEY = 'tickets-page-size';
const savedPageSize = localStorage.getItem(PAGESIZE_STORAGE_KEY);
const defaultPageSize = savedPageSize !== null ? parseInt(savedPageSize) : 0;
const initialPageSize = (urlParams.pageSize && typeof urlParams.pageSize === 'string') ? parseInt(urlParams.pageSize) : defaultPageSize;

const initialSortField = (urlParams.sortField && typeof urlParams.sortField === 'string') ? urlParams.sortField : 'id';
const initialSortDirection = (urlParams.sortDirection && typeof urlParams.sortDirection === 'string') ? urlParams.sortDirection as 'asc' | 'desc' : 'desc';

// Create ticket handler for mobile search bar
const handleCreateTicket = async () => {
  try {
    const newTicket = await ticketService.createEmptyTicket();
    router.push(`/tickets/${newTicket.id}`);
  } catch (error) {
    console.error('Failed to create empty ticket:', error);
  }
};

// Page size change handler that persists preference to localStorage
const handlePageSizeChange = (newSize: number) => {
  localStorage.setItem(PAGESIZE_STORAGE_KEY, String(newSize));
  listManager.handlePageSizeChange(newSize);
};

// List management composable
const listManager = useListManagement<Ticket>({
  itemIdField: 'id',
  defaultSortField: initialSortField,
  defaultSortDirection: initialSortDirection,
  fetchFunction: async (params) => {
    const requestKey = `paginated-tickets-page-${params.page}`;
    return await ticketService.getPaginatedTickets({
      page: params.page,
      pageSize: params.pageSize,
      sortField: params.sortField,
      sortDirection: params.sortDirection,
      search: params.search,
      status: params.status,
      priority: params.priority,
      createdAfter: params.createdAfter,
      createdBefore: params.createdBefore,
      createdOn: params.createdOn,
      modifiedAfter: params.modifiedAfter,
      modifiedBefore: params.modifiedBefore,
      modifiedOn: params.modifiedOn
    }, requestKey);
  },
  routeBuilder: (ticket) => `/tickets/${ticket.id}`,
  mobileSearch: {
    placeholder: 'Search tickets...',
    createIcon: 'ticket',
    onCreate: handleCreateTicket
  }
});

// Set initial values
listManager.searchQuery.value = initialSearchQuery;
listManager.filters.value = initialFilters;
listManager.currentPage.value = initialPage;
listManager.pageSize.value = initialPageSize;

// SSE integration for real-time updates
useListSSE<Ticket>({
  hasItem: listManager.hasItem,
  updateItemField: listManager.updateItemField,
  removeItem: listManager.removeItem,
  prependItem: listManager.prependItem,
  eventTypes: { updated: 'ticket-updated', created: 'ticket-created', deleted: 'ticket-deleted' },
  getEventItemId: (data) => (data.data || data).ticket_id,
  itemKey: 'ticket',
  onItemUpdated: (data) => {
    const { ticket_id, field, value } = data.data || data;
    // Handle user fields that include nested user_info
    if ((field === 'assignee' || field === 'requester') && value?.user_info) {
      listManager.updateItemField(ticket_id, field, value.uuid || value);
      listManager.updateItemField(ticket_id, `${field}_user` as keyof Ticket, value.user_info);
    } else {
      listManager.updateItemField(ticket_id, field, value);
    }
  }
});

// Determine which scroll container to use based on viewport
const activeScrollContainer = computed(() =>
  isMobile.value ? mobileScrollContainer.value : desktopScrollContainer.value
);

// Infinite scroll - uses the active container
useInfiniteScroll({
  containerRef: computed(() => activeScrollContainer.value),
  enabled: listManager.isInfiniteMode,
  hasMore: listManager.hasMore,
  isLoading: listManager.loadingMore,
  onLoadMore: listManager.loadMore
});

// Update URL when state changes (without triggering navigation)
watch(
  [
    () => listManager.searchQuery.value,
    () => listManager.filters.value,
    () => listManager.currentPage.value,
    () => listManager.pageSize.value,
    () => listManager.sortField.value,
    () => listManager.sortDirection.value
  ],
  () => {
    const query: Record<string, string> = {};

    if (listManager.searchQuery.value) {
      query.search = listManager.searchQuery.value;
    }

    // Add filters
    Object.entries(listManager.filters.value).forEach(([key, value]) => {
      if (Array.isArray(value)) {
        if (value.length > 0) {
          query[key] = value.join(',');
        }
      } else if (value && value !== 'all') {
        query[key] = value;
      }
    });

    // Add pagination (only if not default)
    if (listManager.currentPage.value > 1) {
      query.page = listManager.currentPage.value.toString();
    }
    if (listManager.pageSize.value !== 25) {
      query.pageSize = listManager.pageSize.value.toString();
    }

    // Add sorting (only if not default)
    if (listManager.sortField.value !== 'id') {
      query.sortField = listManager.sortField.value;
    }
    if (listManager.sortDirection.value !== 'desc') {
      query.sortDirection = listManager.sortDirection.value;
    }

    const queryString = new URLSearchParams(query).toString();
    const newUrl = queryString ? `${route.path}?${queryString}` : route.path;
    window.history.replaceState(window.history.state, '', newUrl);
  },
  { deep: true }
);

// Table columns
const columns = [
  { field: 'id', label: 'ID', width: 'minmax(60px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'title', label: 'Title', width: '1fr', sortable: true, responsive: 'always' as const },
  { field: 'status', label: 'Status', width: 'minmax(85px,auto)', sortable: true, responsive: 'always' as const },
  { field: 'priority', label: 'Priority', width: 'minmax(75px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'created', label: 'Created', width: 'minmax(90px,auto)', sortable: true, sortKey: 'created_at', responsive: 'lg' as const },
  { field: 'requester', label: 'Requester', width: 'minmax(120px,auto)', sortable: true, sortKey: 'requester_uuid', responsive: 'lg' as const },
  { field: 'assignee', label: 'Assignee', width: 'minmax(120px,auto)', sortable: true, sortKey: 'assignee_uuid', responsive: 'lg' as const }
];

// Filter options
const filterOptions = computed(() => {
  return listManager.buildFilterOptions({
    status: {
      options: STATUS_OPTIONS,
      width: 'w-[130px]',
      allLabel: 'All Statuses',
      placeholder: 'Status',
      multiple: true
    },
    priority: {
      options: PRIORITY_OPTIONS,
      width: 'w-[130px]',
      allLabel: 'All Priorities',
      placeholder: 'Priority'
    }
  });
});


// Staggered fade-in animation
const { getStyle } = useStaggeredList();
</script>

<template>
  <div class="flex flex-col h-full overflow-hidden">
    <!-- Search and filter bar -->
    <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
      <div class="p-2 flex items-center gap-2 flex-wrap">
        <!-- Search input - hidden on mobile (shown in MobileSearchBar) -->
        <DebouncedSearchInput
          :model-value="listManager.searchQuery.value"
          @update:model-value="listManager.handleSearchUpdate"
          placeholder="Search tickets..."
          class="hidden sm:block"
        />

        <!-- Filters -->
        <template v-if="filterOptions.length > 0">
          <div
            v-for="filter in filterOptions"
            :key="filter.name"
            :class="[filter.width || 'w-[120px]']"
          >
            <BaseDropdown
              :model-value="filter.value"
              :options="filter.options"
              :multiple="filter.multiple"
              :placeholder="filter.placeholder"
              size="sm"
              @update:model-value="value => listManager.handleFilterUpdate(filter.name, value)"
            />
          </div>

          <button
            @click="listManager.resetFilters"
            class="px-2 py-1 text-xs font-medium text-white bg-accent rounded-md hover:opacity-90 focus:ring-2 focus:outline-none focus:ring-accent"
          >
            Reset
          </button>
        </template>

        <div v-if="!listManager.isInfiniteMode.value" class="text-xs text-tertiary ml-auto">
          {{ listManager.totalItems.value }} result{{ listManager.totalItems.value !== 1 ? "s" : "" }}
        </div>
      </div>
    </div>

    <!-- Bulk Actions Bar -->
    <BulkActionsBar
      :selected-count="listManager.selectedItems.value.length"
      :total-count="listManager.totalItems.value"
      :actions="bulkActions"
      item-label="ticket"
      @action="handleBulkAction"
      @clear-selection="listManager.clearSelection"
      @select-all="listManager.selectAll"
    />

    <!-- Main content -->
    <div class="flex-1 flex flex-col overflow-hidden">
      <BaseListView
        title="Tickets"
        :is-loading="listManager.loading.value"
        :is-empty="listManager.items.value.length === 0 && !listManager.loading.value"
        :error="listManager.error.value"
        :is-mobile="isMobile"
        :is-loading-more="listManager.loadingMore.value"
        empty-icon="ticket"
        :empty-message="listManager.searchQuery.value ? 'No tickets match your search' : 'No tickets found'"
        :empty-description="listManager.searchQuery.value ? 'Try adjusting your search or filters' : 'Create your first ticket to get started'"
        @retry="listManager.fetchItems"
      >
        <!-- Desktop Table View -->
        <template #default>
          <div
            ref="desktopScrollContainer"
            class="flex-1 overflow-y-auto"
          >
            <DataTable
              :columns="columns"
              :data="listManager.items.value"
              :selected-items="listManager.selectedItems.value"
              :sort-field="listManager.sortField.value"
              :sort-direction="listManager.sortDirection.value"
              :loading="listManager.isBackgroundRefresh.value"
              @update:sort="listManager.handleSortUpdate"
              @toggle-selection="listManager.toggleSelection"
              @toggle-all="listManager.toggleAllItems"
              @row-click="listManager.navigateToItem"
            >
              <template #cell-id="{ value }">
                <IdCell :id="value" />
              </template>

              <template #cell-title="{ value }">
                <TextCell :value="value" font-weight="medium" />
              </template>

              <template #cell-status="{ value }">
                <StatusBadge type="status" :value="value" :short="true" :compact="true" />
              </template>

              <template #cell-priority="{ value }">
                <StatusBadge type="priority" :value="value" :short="true" :compact="true" />
              </template>

              <template #cell-created="{ value }">
                <DateCell :value="value" format="compact" />
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

            <!-- Loading indicator for infinite scroll -->
            <div v-if="listManager.loadingMore.value" class="py-4 flex justify-center bg-app">
              <div class="animate-spin rounded-full h-6 w-6 border-t-2 border-b-2 border-accent"></div>
            </div>
          </div>
        </template>

        <!-- Mobile Card View -->
        <template #mobile-view>
          <div
            ref="mobileScrollContainer"
            class="flex-1 overflow-y-auto"
          >
            <TransitionGroup
              name="list-stagger"
              tag="div"
              class="flex flex-col"
            >
              <div
                v-for="(ticket, index) in listManager.items.value"
                :key="ticket.id"
                :style="getStyle(index)"
                v-memo="[ticket.id, ticket.title, ticket.status, ticket.priority, ticket.created, ticket.requester, ticket.assignee, themeStore.colorBlindMode]"
                @click="listManager.navigateToItem(ticket)"
                :class="[
                  'flex items-center gap-3 px-3 py-2.5 hover:bg-surface-hover active:bg-surface-alt transition-colors cursor-pointer',
                  index > 0 ? 'border-t border-default' : ''
                ]"
              >
                <!-- Status indicator bar -->
                <div
                  v-if="themeStore.colorBlindMode"
                  class="w-2 self-stretch rounded-full flex-shrink-0 relative box-border"
                  :class="{
                    'border-2 border-status-open bg-transparent': ticket.status === 'open',
                    'border-2 border-status-in-progress bg-transparent': ticket.status === 'in-progress',
                    'bg-status-closed': ticket.status === 'closed'
                  }"
                >
                  <div
                    v-if="ticket.status === 'in-progress'"
                    class="absolute inset-x-0 bottom-0 h-1/2 bg-status-in-progress rounded-b-full"
                    style="left: -2px; right: -2px; bottom: -2px;"
                  ></div>
                </div>
                <div
                  v-else
                  class="w-1.5 self-stretch rounded-full flex-shrink-0"
                  :class="{
                    'bg-status-open': ticket.status === 'open',
                    'bg-status-in-progress': ticket.status === 'in-progress',
                    'bg-status-closed': ticket.status === 'closed'
                  }"
                ></div>

                <!-- Main content -->
                <div class="flex-1 min-w-0">
                  <div class="flex items-center gap-2">
                    <span class="text-xs text-secondary font-medium flex-shrink-0">#{{ ticket.id }}</span>
                    <span class="text-sm text-primary font-medium truncate">{{ ticket.title }}</span>
                  </div>

                  <div class="flex flex-wrap items-center gap-x-3 gap-y-1 mt-1.5 text-xs">
                    <div class="flex items-center gap-2 flex-shrink-0">
                      <StatusBadge type="status" :value="ticket.status" :short="true" :compact="true" />
                      <StatusBadge type="priority" :value="ticket.priority" :short="true" :compact="true" />
                    </div>

                    <span class="text-tertiary flex-shrink-0">{{ formatCompactDateTime(ticket.created) }}</span>

                    <div class="flex items-center gap-1 min-w-0">
                      <span class="text-tertiary flex-shrink-0">From:</span>
                      <div class="flex items-center gap-1 min-w-0">
                        <div class="flex-shrink-0 [&>div]:!w-4 [&>div]:!h-4 [&>div>*]:!w-4 [&>div>*]:!h-4 [&>div>*]:!text-[8px]">
                          <UserAvatar
                            v-if="ticket.requester_user?.uuid || ticket.requester"
                            :name="ticket.requester_user?.uuid || ticket.requester"
                            :userName="ticket.requester_user?.name"
                            :avatar="ticket.requester_user?.avatar_thumb"
                            size="xs"
                            :showName="false"
                            :clickable="false"
                          />
                        </div>
                        <span class="text-secondary truncate max-w-[120px]">{{ ticket.requester_user?.name || ticket.requester || 'Unknown' }}</span>
                      </div>
                    </div>

                    <div class="flex items-center gap-1 min-w-0">
                      <span class="text-tertiary flex-shrink-0">To:</span>
                      <div class="flex items-center gap-1 min-w-0">
                        <template v-if="ticket.assignee_user?.name || ticket.assignee">
                          <div class="flex-shrink-0 [&>div]:!w-4 [&>div]:!h-4 [&>div>*]:!w-4 [&>div>*]:!h-4 [&>div>*]:!text-[8px]">
                            <UserAvatar
                              :name="ticket.assignee_user?.uuid || ticket.assignee"
                              :userName="ticket.assignee_user?.name"
                              :avatar="ticket.assignee_user?.avatar_thumb"
                              size="xs"
                              :showName="false"
                              :clickable="false"
                            />
                          </div>
                          <span class="text-secondary truncate max-w-[120px]">{{ ticket.assignee_user?.name || ticket.assignee }}</span>
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
            </TransitionGroup>

            <!-- Loading indicator for infinite scroll -->
            <div v-if="listManager.loadingMore.value" class="py-4 flex justify-center bg-app">
              <div class="animate-spin rounded-full h-6 w-6 border-t-2 border-b-2 border-accent"></div>
            </div>
          </div>
        </template>
      </BaseListView>
    </div>

    <!-- Pagination Controls (visible on desktop, or mobile in pagination mode) -->
    <PaginationControls
      v-if="!isMobile || !listManager.isInfiniteMode.value"
      :current-page="listManager.currentPage.value"
      :total-pages="listManager.totalPages.value"
      :total-items="listManager.totalItems.value"
      :page-size="listManager.pageSize.value"
      :page-size-options="listManager.pageSizeOptions"
      :is-infinite-mode="listManager.isInfiniteMode.value"
      @update:current-page="listManager.handlePageChange"
      @update:page-size="handlePageSizeChange"
      @go-to-item="handleGoToItem"
    />

    <!-- Bulk Status Modal -->
    <Modal
      :show="showStatusModal"
      title="Set Status"
      size="sm"
      @close="showStatusModal = false"
    >
      <div class="flex flex-col gap-2 p-4">
        <p class="text-sm text-secondary mb-2">
          Update status for {{ listManager.selectedItems.value.length }} ticket{{ listManager.selectedItems.value.length !== 1 ? 's' : '' }}
        </p>
        <button
          v-for="status in STATUS_OPTIONS"
          :key="status.value"
          @click="handleBulkStatusChange(status.value)"
          :disabled="bulkActionLoading"
          class="flex items-center gap-3 px-4 py-3 rounded-lg hover:bg-surface-hover transition-colors text-left"
        >
          <StatusBadge type="status" :value="status.value" />
          <span class="text-primary">{{ status.label }}</span>
        </button>
      </div>
    </Modal>

    <!-- Bulk Priority Modal -->
    <Modal
      :show="showPriorityModal"
      title="Set Priority"
      size="sm"
      @close="showPriorityModal = false"
    >
      <div class="flex flex-col gap-2 p-4">
        <p class="text-sm text-secondary mb-2">
          Update priority for {{ listManager.selectedItems.value.length }} ticket{{ listManager.selectedItems.value.length !== 1 ? 's' : '' }}
        </p>
        <button
          v-for="priority in PRIORITY_OPTIONS"
          :key="priority.value"
          @click="handleBulkPriorityChange(priority.value)"
          :disabled="bulkActionLoading"
          class="flex items-center gap-3 px-4 py-3 rounded-lg hover:bg-surface-hover transition-colors text-left"
        >
          <StatusBadge type="priority" :value="priority.value" />
          <span class="text-primary">{{ priority.label }}</span>
        </button>
      </div>
    </Modal>

    <!-- Bulk Assign Modal -->
    <UserSelectionModal
      :show="showAssignModal"
      title="Assign Tickets"
      @close="showAssignModal = false"
      @select="handleBulkAssign"
    />
  </div>
</template>

<style scoped>
.overflow-y-auto::-webkit-scrollbar {
  width: 8px;
}

.overflow-y-auto::-webkit-scrollbar-track {
  background: var(--color-bg-surface);
}

.overflow-y-auto::-webkit-scrollbar-thumb {
  background: var(--color-border-default);
  border-radius: 4px;
}

.overflow-y-auto::-webkit-scrollbar-thumb:hover {
  background: var(--color-border-strong);
}
</style>
