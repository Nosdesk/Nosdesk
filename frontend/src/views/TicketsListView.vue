// views/TicketsListView.vue
<script setup lang="ts">
import { ref, computed, watch, onActivated, onDeactivated, onMounted, onUnmounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
  useInfiniteQuery,
  useMutation,
  useQueryCache,
} from "@pinia/colada";
import { useSSE } from "@/services/sseService";
import ticketService from "@/services/ticketService";
import PageScroll from "@/components/common/PageScroll.vue";
import EmptyState from "@/components/common/EmptyState.vue";
import ErrorBanner from "@/components/common/ErrorBanner.vue";
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
import { useListControls } from "@/composables/useListControls";
import { useMobileSearch } from "@/composables/useMobileSearch";
import { useStaggeredList } from "@/composables/useStaggeredList";
import { useMobileDetection } from "@/composables/useMobileDetection";
import { useInfiniteScroll } from "@/composables/useInfiniteScroll";
import { useTicketsListLoader } from "@/loaders/ticketsListLoader";
import { useThemeStore } from "@/stores/theme";
import { useAuthStore } from "@/stores/auth";
import { parseDate } from "@/utils/dateUtils";
import { STATUS_OPTIONS, PRIORITY_OPTIONS } from "@/constants/ticketOptions";
import { categoryService } from "@/services/categoryService";
import type { TicketCategory } from "@/types/category";
import { ticketsKeys } from "@/queries/tickets";
import type { Ticket } from "@/services/ticketService";

defineOptions({ name: 'TicketsListView' })

const themeStore = useThemeStore();
const route = useRoute();
const router = useRouter();

// Shared mobile detection (lg breakpoint = 1024px)
const { isMobile } = useMobileDetection('lg');

// PageScroll owns the scroll container; we read its exposed
// ref to feed `useInfiniteScroll`. One scroll surface for both
// the desktop table and the mobile cards now — they share the
// same scroll region instead of each having their own.
const pageScrollRef = ref<InstanceType<typeof PageScroll> | null>(null);
const scrollContainerRef = computed<HTMLElement | null>(
  () => pageScrollRef.value?.scrollContainerRef ?? null,
);

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

// Bulk action mutation. One mutation handles all four bulk
// actions; the action discriminator + optional value live on the
// vars. Pinia Colada tracks pending state per-instance and
// contributes to the global progress bar automatically.
const bulkActionMutation = useMutation({
  mutation: (vars: {
    action: 'delete' | 'set-status' | 'set-priority' | 'assign'
    ids: number[]
    value?: string
  }) => ticketService.bulkAction(vars),
  onSettled: () => {
    // Server-authoritative for bulk operations: re-fetch instead
    // of trying to optimistically reconcile a multi-row change
    // across infinite + paginated cache entries.
    queryCache.invalidateQueries({ key: ticketsKeys.root })
  },
  onError: (err) => {
    console.error('Bulk action failed:', err)
    alert('Failed to perform bulk action. Please try again.')
  },
})
const bulkActionLoading = computed(() => bulkActionMutation.asyncStatus.value === 'loading')

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

const executeBulkAction = async (
  action: 'delete' | 'set-status' | 'set-priority' | 'assign',
  value?: string
) => {
  const ids = controls.selectedItems.value.map(id => parseInt(id));
  if (ids.length === 0) return;
  await bulkActionMutation.mutateAsync({ action, ids, value })
  controls.clearSelection();
  showStatusModal.value = false;
  showPriorityModal.value = false;
  showAssignModal.value = false;
};

const handleBulkStatusChange = (status: string) => {
  executeBulkAction('set-status', status);
};

const handleBulkPriorityChange = (priority: string) => {
  executeBulkAction('set-priority', priority);
};

const handleBulkAssign = (userId: string) => {
  executeBulkAction('assign', userId);
  showAssignModal.value = false;
};

// Extract URL params for initial state
const urlParams = route.query;
const initialFilters: Record<string, string | string[]> = {};

// Define which filters support multiple values
const multiSelectFilters = ['status'];

// Get current user UUID for 'current' filter value
const authStore = useAuthStore();
const currentUserUuid = computed(() => authStore.user?.uuid || '');

// Categories for filter
const categories = ref<TicketCategory[]>([]);
const loadCategories = async () => {
  try {
    categories.value = await categoryService.getCategories();
  } catch (err) {
    console.error('Failed to load categories:', err);
  }
};
loadCategories();

// Set initial values from URL
const filterKeys = ['status', 'priority', 'category', 'assignee', 'requester', 'createdOn', 'createdAfter', 'createdBefore',
                    'modifiedOn', 'modifiedAfter', 'modifiedBefore', 'closedOn', 'closedAfter', 'closedBefore'];
filterKeys.forEach(key => {
  if (urlParams[key] && typeof urlParams[key] === 'string') {
    let value = urlParams[key] as string;
    // Handle 'current' as a special value meaning current user
    if ((key === 'assignee' || key === 'requester') && value === 'current') {
      value = currentUserUuid.value;
    }
    // 'unassigned' is passed through as-is for backend to filter by null assignee
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
  controls.handlePageSizeChange(newSize);
};

// UI-state composable. Owns filters, sort, search, selection,
// pagination state. No data side effects: the data layer below
// reads from `controls.requestParams` via a reactive query key
// and Pinia Colada handles refetch automatically when filters
// or sort change.
const controls = useListControls<Ticket>({
  itemIdField: 'id',
  defaultSortField: initialSortField,
  defaultSortDirection: initialSortDirection,
  initialSearch: initialSearchQuery,
  initialFilters,
  initialPage,
  initialPageSize,
})

// Subscribe to the route Data Loader. The loader has already
// pre-fetched the first page during navigation and primed the
// matching infinite-query cache entry, so the queries below
// resolve from cache without firing fresh requests on mount.
useTicketsListLoader()

// Pinia Colada query layer.
//
// Tickets supports two pagination modes: infinite scroll
// (pageSize = 0, append loaded pages) and paged (pageSize = 25,
// 50, 100, jump to page N). We use one `useInfiniteQuery` for
// each mode, keyed differently, with `enabled` toggling on the
// active mode. The inactive mode's cache is harmless idle data.
const queryCache = useQueryCache()

const infiniteList = useInfiniteQuery(() => ({
  // Page is NOT in the key for infinite mode (pages append into
  // one cache entry per filter set). Filter / sort / search ARE.
  key: ticketsKeys.list('infinite', controls.cacheKeyPart.value),
  initialPageParam: 1,
  query: ({ pageParam }) =>
    ticketService.getPaginatedTickets(
      { ...controls.requestParams.value, page: pageParam },
      `tickets-infinite-page-${pageParam}`,
    ),
  getNextPageParam: (lastPage, allPages) =>
    allPages.length < lastPage.totalPages ? allPages.length + 1 : null,
  enabled: () => controls.isInfiniteMode.value,
}))

const paginatedList = useInfiniteQuery(() => ({
  // Page IS in the key here: each page is a discrete cache entry
  // so jumping from page 5 to page 1 doesn't re-fetch page 5.
  key: ticketsKeys.list('paginated', controls.cacheKeyPart.value, controls.currentPage.value),
  initialPageParam: controls.currentPage.value,
  query: ({ pageParam }) =>
    ticketService.getPaginatedTickets(
      { ...controls.requestParams.value, page: pageParam },
      `tickets-paginated-page-${pageParam}`,
    ),
  // Paginated mode never appends; "load more" is disabled by
  // `getNextPageParam` returning null.
  getNextPageParam: () => null,
  enabled: () => !controls.isInfiniteMode.value,
}))

const items = computed<Ticket[]>(() => {
  const source = controls.isInfiniteMode.value ? infiniteList : paginatedList
  return source.data.value?.pages.flatMap(p => p.data) ?? []
})

const totalItems = computed(() => {
  const source = controls.isInfiniteMode.value ? infiniteList : paginatedList
  return source.data.value?.pages[0]?.total ?? 0
})

const totalPages = computed(() => {
  const source = controls.isInfiniteMode.value ? infiniteList : paginatedList
  return source.data.value?.pages[0]?.totalPages ?? 1
})

const hasMore = computed(() =>
  controls.isInfiniteMode.value ? infiniteList.hasNextPage.value : false,
)

const activeAsyncStatus = computed(() =>
  controls.isInfiniteMode.value
    ? infiniteList.asyncStatus.value
    : paginatedList.asyncStatus.value,
)

const activeError = computed(() =>
  controls.isInfiniteMode.value ? infiniteList.error.value : paginatedList.error.value,
)

const isFetching = computed(() => activeAsyncStatus.value === 'loading')
const isFirstLoad = computed(() => isFetching.value && items.value.length === 0)
const isLoadingMore = computed(() => isFetching.value && items.value.length > 0)
const isBackgroundRefresh = computed(() => isFetching.value && items.value.length > 0)

// SSE-driven cache invalidation. Keep it dumb: any ticket
// updated/created/deleted invalidates all tickets list queries
// (across both modes and all loaded pages). Pinia Colada will
// refetch only the active query. This is simpler than
// surgically mutating per-page cache entries; revisit if perf
// shows it's an issue.
const sse = useSSE()
function invalidateTicketsList() {
  queryCache.invalidateQueries({ key: ticketsKeys.root })
}

const sseHandlers: Array<{ type: string; handler: (data: unknown) => void }> = [
  { type: 'ticket-updated', handler: invalidateTicketsList },
  { type: 'ticket-created', handler: invalidateTicketsList },
  { type: 'ticket-deleted', handler: invalidateTicketsList },
]

onMounted(() => {
  if (!sse.isConnected.value) sse.connect()
  for (const { type, handler } of sseHandlers) {
    sse.addEventListener(type as never, handler)
  }
})
onUnmounted(() => {
  for (const { type, handler } of sseHandlers) {
    sse.removeEventListener(type as never, handler)
  }
})

const navigateToTicket = (ticket: Ticket) => {
  router.push(`/tickets/${ticket.id}`)
}

// Selection / retry handlers that need access to `items`. The
// composable's selection helpers take items as an argument so
// they stay data-agnostic.
const handleToggleSelection = (event: Event, itemId: string) =>
  controls.toggleSelection(event, itemId, items.value)
const handleToggleAll = (event: Event) =>
  controls.toggleAllItems(event, items.value)
const handleSelectAll = () => controls.selectAll(items.value)
const handleRetry = () => {
  if (controls.isInfiniteMode.value) infiniteList.refetch()
  else paginatedList.refetch()
}
const errorMessage = computed(() => {
  if (!activeError.value) return null
  return activeError.value instanceof Error
    ? activeError.value.message
    : 'Failed to load tickets. Please try again.'
})

// Helper to parse URL query into filters
const parseUrlFilters = (query: typeof route.query): Record<string, string | string[]> => {
  const filters: Record<string, string | string[]> = {};
  filterKeys.forEach(key => {
    if (query[key] && typeof query[key] === 'string') {
      let value = query[key] as string;
      // Handle 'current' as a special value meaning current user
      if ((key === 'assignee' || key === 'requester') && value === 'current') {
        value = currentUserUuid.value;
      }
      // 'unassigned' is passed through as-is for backend to filter by null assignee
      if (multiSelectFilters.includes(key) && value.includes(',')) {
        filters[key] = value.split(',');
      } else if (multiSelectFilters.includes(key)) {
        filters[key] = [value];
      } else {
        filters[key] = value;
      }
    }
  });
  return filters;
};

// Watch for route query changes (e.g., clicking dashboard stats).
// Pinia Colada handles refetch automatically via the reactive
// query key once `controls.filters.value` / `controls.searchQuery.value`
// update.
watch(() => route.query, (newQuery) => {
  const newFilters = parseUrlFilters(newQuery);
  const newSearch = (newQuery.search && typeof newQuery.search === 'string') ? newQuery.search : '';

  const filtersChanged = JSON.stringify(newFilters) !== JSON.stringify(controls.filters.value);
  const searchChanged = newSearch !== controls.searchQuery.value;

  if (filtersChanged) controls.filters.value = newFilters;
  if (searchChanged) controls.searchQuery.value = newSearch;
}, { deep: true });

// Re-apply URL filters when component is activated from KeepAlive cache
onActivated(() => {
  const currentFilters = parseUrlFilters(route.query);
  const currentSearch = (route.query.search && typeof route.query.search === 'string') ? route.query.search : '';

  const filtersChanged = JSON.stringify(currentFilters) !== JSON.stringify(controls.filters.value);
  const searchChanged = currentSearch !== controls.searchQuery.value;

  if (filtersChanged) controls.filters.value = currentFilters;
  if (searchChanged) controls.searchQuery.value = currentSearch;
});

// Mobile search bar registration (was inlined in useListManagement
// before, now wired explicitly so the migration doesn't lose it).
const mobileSearch = useMobileSearch()
function setupMobileSearch() {
  mobileSearch.registerMobileSearch({
    searchQuery: controls.searchQuery.value,
    placeholder: 'Search tickets...',
    showCreateButton: true,
    createIcon: 'ticket',
    onSearchUpdate: controls.handleSearchUpdate,
    onCreate: handleCreateTicket,
  })
}
onMounted(setupMobileSearch)
onActivated(setupMobileSearch)
onDeactivated(mobileSearch.deregisterMobileSearch)
onUnmounted(mobileSearch.deregisterMobileSearch)
watch(controls.searchQuery.value, mobileSearch.updateSearchQuery)

// Infinite scroll - PageScroll's single scroll container backs
// both desktop and mobile views.
useInfiniteScroll({
  containerRef: scrollContainerRef,
  enabled: controls.isInfiniteMode.value,
  hasMore,
  isLoading: computed(() => isLoadingMore.value),
  onLoadMore: () => infiniteList.loadNextPage(),
});

// Update URL when state changes (without triggering navigation)
watch(
  [
    () => controls.searchQuery.value,
    () => controls.filters.value,
    () => controls.currentPage.value,
    () => controls.pageSize.value,
    () => controls.sortField.value,
    () => controls.sortDirection.value
  ],
  () => {
    const query: Record<string, string> = {};

    if (controls.searchQuery.value) {
      query.search = controls.searchQuery.value;
    }

    Object.entries(controls.filters.value).forEach(([key, value]) => {
      if (Array.isArray(value)) {
        if (value.length > 0) {
          query[key] = value.join(',');
        }
      } else if (value && value !== 'all') {
        query[key] = value;
      }
    });

    if (controls.currentPage.value > 1) {
      query.page = controls.currentPage.value.toString();
    }
    if (controls.pageSize.value !== 25) {
      query.pageSize = controls.pageSize.value.toString();
    }
    if (controls.sortField.value !== 'id') {
      query.sortField = controls.sortField.value;
    }
    if (controls.sortDirection.value !== 'desc') {
      query.sortDirection = controls.sortDirection.value;
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
  const categoryOptions = categories.value.map(cat => ({
    value: String(cat.id),
    label: cat.name
  }));

  return controls.buildFilterOptions({
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
    },
    category: {
      options: categoryOptions,
      width: 'w-[140px]',
      allLabel: 'All Categories',
      placeholder: 'Category'
    }
  });
});


// Staggered fade-in animation
const { getStyle } = useStaggeredList();

// Expose methods for parent component access (SiteHeader create button)
defineExpose({
  handleCreateTicket
});
</script>

<template>
  <PageScroll
    ref="pageScrollRef"
    content-class="flex h-full flex-col"
    :is-empty="!!errorMessage || (items.length === 0 && !isFirstLoad)"
  >
    <template #chrome>
      <!-- Search and filter bar -->
      <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
        <div class="p-2 flex items-center gap-2 flex-wrap">
          <DebouncedSearchInput
            :model-value="controls.searchQuery.value"
            @update:model-value="controls.handleSearchUpdate"
            placeholder="Search tickets..."
            class="hidden sm:block"
          />

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
                @update:model-value="value => controls.handleFilterUpdate(filter.name, value)"
              />
            </div>

            <button
              @click="controls.resetFilters"
              class="px-2 py-1 text-xs font-medium text-white bg-accent rounded-md hover:opacity-90 focus:ring-2 focus:outline-none focus:ring-accent"
            >
              Reset
            </button>
          </template>

          <div v-if="!controls.isInfiniteMode.value" class="text-xs text-tertiary ml-auto">
            {{ totalItems }} result{{ totalItems !== 1 ? "s" : "" }}
          </div>
        </div>
      </div>

      <BulkActionsBar
        :selected-count="controls.selectedItems.value.length"
        :total-count="totalItems"
        :actions="bulkActions"
        item-label="ticket"
        @action="handleBulkAction"
        @clear-selection="controls.clearSelection"
        @select-all="handleSelectAll"
      />
    </template>

    <!-- Empty / error state. Errors take priority over empty;
         keeping both behind the same slot mirrors what
         BaseListView did, just with the layout positioning
         that PageScroll guarantees. -->
    <template #empty>
      <ErrorBanner
        v-if="errorMessage"
        :message="errorMessage"
        :show-retry="true"
        @retry="handleRetry"
      />
      <EmptyState
        v-else
        icon="ticket"
        :title="controls.searchQuery.value ? 'No tickets match your search' : 'No tickets found'"
        :description="controls.searchQuery.value ? 'Try adjusting your search or filters' : 'Create your first ticket to get started'"
      />
    </template>

    <!-- Desktop / mobile views share the same scroll container
         (PageScroll owns it) and toggle via v-show. The flex
         column wrapper gives DataTable's `h-full` something to
         anchor against (without a height chain DataTable would
         render at 0px and look empty). -->
    <div v-show="!isMobile" class="flex h-full flex-col">
      <DataTable
              :columns="columns"
              :data="items"
              :selected-items="controls.selectedItems.value"
              :sort-field="controls.sortField.value"
              :sort-direction="controls.sortDirection.value"
              :loading="isBackgroundRefresh"
              @update:sort="controls.handleSortUpdate"
              @toggle-selection="handleToggleSelection"
              @toggle-all="handleToggleAll"
              @row-click="navigateToTicket"
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
            <div v-if="isLoadingMore" class="py-4 flex justify-center bg-app">
              <div class="animate-spin rounded-full h-6 w-6 border-t-2 border-b-2 border-accent"></div>
            </div>
    </div>

    <div v-show="isMobile" class="flex h-full flex-col">
      <TransitionGroup
        name="list-stagger"
        tag="div"
        class="flex flex-col"
      >
              <div
                v-for="(ticket, index) in items"
                :key="ticket.id"
                :style="getStyle(index)"
                v-memo="[ticket.id, ticket.title, ticket.status, ticket.priority, ticket.created, ticket.requester, ticket.assignee, themeStore.effectiveColorBlindMode]"
                @click="navigateToTicket(ticket)"
                :class="[
                  'flex items-center gap-3 px-3 py-2.5 hover:bg-surface-hover active:bg-surface-alt transition-colors cursor-pointer',
                  index > 0 ? 'border-t border-default' : ''
                ]"
              >
                <!-- Status indicator bar -->
                <div
                  v-if="themeStore.effectiveColorBlindMode"
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
                        <div class="flex-shrink-0 [&>div]:!w-4 [&>div]:!h-4 [&>div>*]:!w-4 [&>div>*]:!h-4 [&>div>*]:!text-[0.5rem]">
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
                          <div class="flex-shrink-0 [&>div]:!w-4 [&>div]:!h-4 [&>div>*]:!w-4 [&>div>*]:!h-4 [&>div>*]:!text-[0.5rem]">
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
      <div v-if="isLoadingMore" class="py-4 flex justify-center bg-app">
        <div class="animate-spin rounded-full h-6 w-6 border-t-2 border-b-2 border-accent"></div>
      </div>
    </div>

    <!-- Pagination Controls (visible on desktop, or mobile in
         pagination mode). Lives in PageScroll's footer slot so
         it stays anchored below the scroll region. -->
    <template #footer>
      <PaginationControls
        v-if="!isMobile || !controls.isInfiniteMode.value"
        :current-page="controls.currentPage.value"
        :total-pages="totalPages"
        :total-items="totalItems"
        :page-size="controls.pageSize.value"
        :page-size-options="controls.pageSizeOptions"
        :is-infinite-mode="controls.isInfiniteMode.value"
        @update:current-page="controls.handlePageChange"
        @update:page-size="handlePageSizeChange"
        @go-to-item="handleGoToItem"
      />

      <!-- Modals teleport to body — they sit in the footer slot
           so PageScroll remains the single root for clean
           attribute fallthrough from the parent <RouterView>. -->
      <Modal
        :show="showStatusModal"
        title="Set Status"
        size="sm"
        @close="showStatusModal = false"
      >
        <div class="flex flex-col gap-2 p-4">
          <p class="text-sm text-secondary mb-2">
            Update status for {{ controls.selectedItems.value.length }} ticket{{ controls.selectedItems.value.length !== 1 ? 's' : '' }}
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

      <Modal
        :show="showPriorityModal"
        title="Set Priority"
        size="sm"
        @close="showPriorityModal = false"
      >
        <div class="flex flex-col gap-2 p-4">
          <p class="text-sm text-secondary mb-2">
            Update priority for {{ controls.selectedItems.value.length }} ticket{{ controls.selectedItems.value.length !== 1 ? 's' : '' }}
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

      <UserSelectionModal
        :show="showAssignModal"
        title="Assign Tickets"
        @close="showAssignModal = false"
        @select="handleBulkAssign"
      />
    </template>
  </PageScroll>
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
