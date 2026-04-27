<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useInfiniteQuery, useMutation, useQueryCache } from "@pinia/colada";
import { useSSE } from "@/services/sseService";
import PageScroll from "@/components/common/PageScroll.vue";
import EmptyState from "@/components/common/EmptyState.vue";
import ErrorBanner from "@/components/common/ErrorBanner.vue";
import DataTable from "@/components/common/DataTable.vue";
import DebouncedSearchInput from "@/components/common/DebouncedSearchInput.vue";
import PaginationControls from "@/components/common/PaginationControls.vue";
import BulkActionsBar from "@/components/common/BulkActionsBar.vue";
import type { BulkAction } from "@/components/common/BulkActionsBar.vue";
import Modal from "@/components/Modal.vue";
import { StatusBadgeCell, UserInfoCell, DateCell } from "@/components/common/cells";
import UserAvatar from "@/components/UserAvatar.vue";
import BaseDropdown from "@/components/common/BaseDropdown.vue";
import { useListControls } from "@/composables/useListControls";
import { useMobileSearch } from "@/composables/useMobileSearch";
import { useStaggeredList } from "@/composables/useStaggeredList";
import { useMobileDetection } from "@/composables/useMobileDetection";
import { useInfiniteScroll } from "@/composables/useInfiniteScroll";
import { useDataStore } from "@/stores/dataStore";
import userService from "@/services/userService";
import type { User } from "@/types/user";

const USERS_KEYS = {
  root: ['users'] as const,
  list: (variant: 'infinite' | 'paginated', cacheKey: string, page?: number) =>
    page === undefined
      ? ([...USERS_KEYS.root, 'list', variant, cacheKey] as const)
      : ([...USERS_KEYS.root, 'list', variant, cacheKey, page] as const),
}

defineOptions({ name: 'UsersListView' })

const router = useRouter();
const dataStore = useDataStore();
const queryCache = useQueryCache();

const { isMobile } = useMobileDetection();

const pageScrollRef = ref<InstanceType<typeof PageScroll> | null>(null);
const scrollContainerRef = computed<HTMLElement | null>(
  () => pageScrollRef.value?.scrollContainerRef ?? null,
);

const navigateToCreateUser = () => {
  router.push('/users/new');
};

const navigateToUser = (user: User) => {
  router.push(`/users/${user.uuid}`);
};

// UI-state composable. Pure local state, no data side effects.
const controls = useListControls<User>({
  itemIdField: 'uuid',
  defaultSortField: 'name',
  defaultSortDirection: 'asc',
  defaultPageSize: 0,
})

// Pinia Colada query layer. Dual-mode (infinite + paged), one
// query is enabled at a time based on `controls.isInfiniteMode`.
const infiniteList = useInfiniteQuery(() => ({
  key: USERS_KEYS.list('infinite', controls.cacheKeyPart.value),
  initialPageParam: 1,
  query: ({ pageParam }) => dataStore.getPaginatedUsers({
    ...controls.requestParams.value,
    page: pageParam,
  }),
  getNextPageParam: (lastPage, allPages) =>
    allPages.length < lastPage.totalPages ? allPages.length + 1 : null,
  enabled: () => controls.isInfiniteMode.value,
}))

const paginatedList = useInfiniteQuery(() => ({
  key: USERS_KEYS.list('paginated', controls.cacheKeyPart.value, controls.currentPage.value),
  initialPageParam: controls.currentPage.value,
  query: ({ pageParam }) => dataStore.getPaginatedUsers({
    ...controls.requestParams.value,
    page: pageParam,
  }),
  getNextPageParam: () => null,
  enabled: () => !controls.isInfiniteMode.value,
}))

const items = computed<User[]>(() => {
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

// SSE-driven cache invalidation. Same dumb-and-correct pattern
// as the tickets view: any user updated/created/deleted
// invalidates the users root key, Pinia Colada refetches just
// the active query.
const sse = useSSE()
function invalidateUsersList() {
  queryCache.invalidateQueries({ key: USERS_KEYS.root })
}

const sseHandlers: Array<{ type: string; handler: (data: unknown) => void }> = [
  { type: 'user-updated', handler: invalidateUsersList },
  { type: 'user-created', handler: invalidateUsersList },
  { type: 'user-deleted', handler: invalidateUsersList },
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

// Mobile search bar wiring (was inlined in useListManagement).
const mobileSearch = useMobileSearch()
function setupMobileSearch() {
  mobileSearch.registerMobileSearch({
    searchQuery: controls.searchQuery.value,
    placeholder: 'Search users...',
    showCreateButton: true,
    createIcon: 'user',
    onSearchUpdate: controls.handleSearchUpdate,
    onCreate: navigateToCreateUser,
  })
}
onMounted(setupMobileSearch)
onUnmounted(mobileSearch.deregisterMobileSearch)
watch(controls.searchQuery, mobileSearch.updateSearchQuery)

// Infinite scroll uses PageScroll's single scroll container.
useInfiniteScroll({
  containerRef: scrollContainerRef,
  enabled: controls.isInfiniteMode,
  hasMore,
  isLoading: computed(() => isLoadingMore.value),
  onLoadMore: () => infiniteList.loadNextPage(),
})

// Define table columns with responsive behavior
const columns = [
  { field: 'user', label: 'User', width: '1fr', sortable: true, sortKey: 'name', responsive: 'always' as const },
  { field: 'role', label: 'Role', width: 'minmax(100px,auto)', sortable: true, responsive: 'always' as const },
  { field: 'open_ticket_count', label: 'Tickets', width: 'minmax(80px,auto)', sortable: false, responsive: 'md' as const },
  { field: 'device_count', label: 'Devices', width: 'minmax(80px,auto)', sortable: false, responsive: 'md' as const },
  { field: 'created_at', label: 'Joined', width: 'minmax(140px,auto)', sortable: false, responsive: 'lg' as const }
];

// Build filter options - role is the only available filter from the API
const filterOptions = computed(() => controls.buildFilterOptions({
  role: {
    options: [
      { value: 'admin', label: 'Admin' },
      { value: 'technician', label: 'Technician' },
      { value: 'user', label: 'User' }
    ],
    width: 'w-[140px]',
    allLabel: 'All Roles'
  }
}));

// Custom grid template for responsive layout (includes checkbox column with auto width)
const gridClass = "grid-cols-[auto_1fr_minmax(100px,auto)] md:grid-cols-[auto_1fr_minmax(100px,auto)_minmax(80px,auto)_minmax(80px,auto)] lg:grid-cols-[auto_1fr_minmax(100px,auto)_minmax(80px,auto)_minmax(80px,auto)_minmax(140px,auto)]";

// Staggered fade-in animation
const { getStyle } = useStaggeredList();

// Role options for bulk role change
const ROLE_OPTIONS = [
  { value: 'admin', label: 'Admin' },
  { value: 'technician', label: 'Technician' },
  { value: 'user', label: 'User' }
];

// Bulk actions configuration
const bulkActions: BulkAction[] = [
  { id: 'set-role', label: 'Role', icon: 'role' },
  { id: 'delete', label: 'Delete', icon: 'delete', variant: 'danger', confirm: true }
];

const showRoleModal = ref(false);

const bulkActionMutation = useMutation({
  mutation: (vars: { action: 'delete' | 'set-role'; ids: string[]; value?: string }) =>
    userService.bulkAction(vars),
  onSettled: () => {
    queryCache.invalidateQueries({ key: USERS_KEYS.root })
  },
  onError: (err) => {
    console.error('Bulk action failed:', err)
    alert('Failed to perform bulk action. Please try again.')
  },
})
const bulkActionLoading = computed(() => bulkActionMutation.asyncStatus.value === 'loading')

const handleBulkAction = async (actionId: string) => {
  if (actionId === 'set-role') {
    showRoleModal.value = true;
  } else if (actionId === 'delete') {
    await executeBulkAction('delete');
  }
};

const executeBulkAction = async (action: 'delete' | 'set-role', value?: string) => {
  const ids = controls.selectedItems.value;
  if (ids.length === 0) return;
  await bulkActionMutation.mutateAsync({ action, ids, value })
  controls.clearSelection();
  showRoleModal.value = false;
};

const handleBulkRoleChange = (role: string) => {
  executeBulkAction('set-role', role);
};

// Handlers that need access to `items` (selection helpers
// stay data-agnostic in the composable).
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
    : 'Failed to load users. Please try again.'
})

// Expose method for parent (App.vue) to call from header button
defineExpose({
  navigateToCreateUser
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
            placeholder="Search users..."
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

          <div class="text-xs text-secondary flex items-center gap-4 ml-auto">
            <span>{{ totalItems }} result{{ totalItems !== 1 ? "s" : "" }}</span>
          </div>
        </div>
      </div>

      <BulkActionsBar
        :selected-count="controls.selectedItems.value.length"
        :total-count="totalItems"
        :actions="bulkActions"
        item-label="user"
        @action="handleBulkAction"
        @clear-selection="controls.clearSelection"
        @select-all="handleSelectAll"
      />
    </template>

    <template #empty>
      <ErrorBanner
        v-if="errorMessage"
        :message="errorMessage"
        :show-retry="true"
        @retry="handleRetry"
      />
      <EmptyState
        v-else
        icon="users"
        :title="controls.searchQuery.value ? 'No users match your search' : 'No users found'"
        :description="controls.searchQuery.value ? 'Try adjusting your search criteria' : 'Invite users to get started'"
        :action-label="!controls.searchQuery.value ? 'Invite User' : undefined"
        @action="navigateToCreateUser"
      />
    </template>

    <div v-show="!isMobile" class="flex h-full flex-col">
      <DataTable
              :columns="columns"
              :data="items"
              :selected-items="controls.selectedItems.value"
              :item-id-field="'uuid'"
              :sort-field="controls.sortField.value"
              :sort-direction="controls.sortDirection.value"
              :grid-class="gridClass"
              @update:sort="controls.handleSortUpdate"
              @toggle-selection="handleToggleSelection"
              @toggle-all="handleToggleAll"
              @row-click="navigateToUser"
            >
            <!-- Custom cell templates -->
            <template #cell-user="{ item }">
              <UserInfoCell
                :user-id="item.uuid"
                :user-name="item.name"
                :email="item.email"
                :avatar="item.avatar_thumb || item.avatar_url"
                :show-avatar="true"
              />
            </template>

            <template #cell-role="{ value }">
              <StatusBadgeCell type="role" :value="value" />
            </template>

            <template #cell-open_ticket_count="{ value }">
              <span class="text-sm text-secondary tabular-nums">{{ value ?? 0 }}</span>
            </template>

            <template #cell-device_count="{ value }">
              <span class="text-sm text-secondary tabular-nums">{{ value ?? 0 }}</span>
            </template>

            <template #cell-created_at="{ value }">
              <DateCell :value="value" format="clean-relative" />
            </template>
      </DataTable>
    </div>

    <div v-show="isMobile" class="flex h-full flex-col">
      <TransitionGroup
        name="list-stagger"
        tag="div"
        class="flex flex-col"
      >
            <div
              v-for="(user, index) in items"
              :key="user.uuid"
              :style="getStyle(index)"
              @click="navigateToUser(user)"
              :class="[
                'flex items-center gap-3 px-3 py-2.5 hover:bg-surface-hover active:bg-surface-alt transition-colors cursor-pointer',
                index > 0 ? 'border-t border-default' : ''
              ]"
            >
              <!-- Avatar -->
              <UserAvatar
                :name="user.uuid"
                :userName="user.name"
                :avatar="user.avatar_thumb || user.avatar_url"
                size="sm"
                :clickable="false"
                :show-name="false"
                class="flex-shrink-0"
              />

              <!-- Main content -->
              <div class="flex-1 min-w-0">
                <!-- Name -->
                <div class="text-sm text-primary font-medium truncate">{{ user.name }}</div>

                <!-- Meta row: email, role, counts -->
                <div class="flex flex-wrap items-center gap-2 mt-1 text-xs">
                  <span v-if="user.email" class="text-tertiary truncate max-w-[200px]">{{ user.email }}</span>
                  <span
                    class="inline-flex items-center px-1.5 py-0.5 rounded font-medium capitalize"
                    :class="{
                      'bg-status-error-muted text-status-error': user.role === 'admin',
                      'bg-accent-muted text-accent': user.role === 'technician',
                      'bg-surface-alt text-secondary': user.role === 'user'
                    }"
                  >
                    {{ user.role }}
                  </span>
                  <span v-if="user.open_ticket_count" class="text-secondary tabular-nums">{{ user.open_ticket_count }} ticket{{ user.open_ticket_count !== 1 ? 's' : '' }}</span>
                  <span v-if="user.device_count" class="text-secondary tabular-nums">{{ user.device_count }} device{{ user.device_count !== 1 ? 's' : '' }}</span>
                </div>
              </div>

              <!-- Chevron -->
              <svg class="w-4 h-4 text-tertiary flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
              </svg>
      </div>
      </TransitionGroup>
    </div>

    <template #footer>
      <PaginationControls
        v-if="!isMobile"
        :current-page="controls.currentPage.value"
        :total-pages="totalPages"
        :page-size="controls.pageSize.value"
        :page-size-options="controls.pageSizeOptions"
        :show-import="true"
        @update:current-page="controls.handlePageChange"
        @update:page-size="controls.handlePageSizeChange"
        @import="() => {}"
      />

      <Modal
        :show="showRoleModal"
        title="Set Role"
        size="sm"
        @close="showRoleModal = false"
      >
        <div class="flex flex-col gap-2 p-4">
          <p class="text-sm text-secondary mb-2">
            Update role for {{ controls.selectedItems.value.length }} user{{ controls.selectedItems.value.length !== 1 ? 's' : '' }}
          </p>
          <button
            v-for="role in ROLE_OPTIONS"
            :key="role.value"
            @click="handleBulkRoleChange(role.value)"
            :disabled="bulkActionLoading"
            class="flex items-center gap-3 px-4 py-3 rounded-lg hover:bg-surface-hover transition-colors text-left"
          >
            <StatusBadgeCell type="role" :value="role.value" />
            <span class="text-primary">{{ role.label }}</span>
          </button>
        </div>
      </Modal>
    </template>
  </PageScroll>
</template>

<style scoped>
/* Custom scrollbar styling */
.overflow-y-auto::-webkit-scrollbar,
.overflow-x-auto::-webkit-scrollbar {
  width: 8px;
  height: 8px;
}

.overflow-y-auto::-webkit-scrollbar-track,
.overflow-x-auto::-webkit-scrollbar-track {
  background: var(--color-bg-surface);
}

.overflow-y-auto::-webkit-scrollbar-thumb,
.overflow-x-auto::-webkit-scrollbar-thumb {
  background: var(--color-border-default);
  border-radius: 4px;
}

.overflow-y-auto::-webkit-scrollbar-thumb:hover,
.overflow-x-auto::-webkit-scrollbar-thumb:hover {
  background: var(--color-border-strong);
}

.overflow-x-auto::-webkit-scrollbar-corner {
  background: var(--color-bg-surface);
}
</style>
