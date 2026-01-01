<script setup lang="ts">
import { ref } from "vue";
import { useRouter } from "vue-router";
import BaseListView from "@/components/common/BaseListView.vue";
import DataTable from "@/components/common/DataTable.vue";
import DebouncedSearchInput from "@/components/common/DebouncedSearchInput.vue";
import PaginationControls from "@/components/common/PaginationControls.vue";
import BulkActionsBar from "@/components/common/BulkActionsBar.vue";
import type { BulkAction } from "@/components/common/BulkActionsBar.vue";
import Modal from "@/components/Modal.vue";
import { StatusBadgeCell, UserInfoCell, DateCell } from "@/components/common/cells";
import UserAvatar from "@/components/UserAvatar.vue";
import { useListManagement } from "@/composables/useListManagement";
import { useListSSE } from "@/composables/useListSSE";
import { useStaggeredList } from "@/composables/useStaggeredList";
import { useMobileDetection } from "@/composables/useMobileDetection";
import { useDataStore } from "@/stores/dataStore";
import userService from "@/services/userService";
import type { User } from "@/types/user";

const router = useRouter();
const dataStore = useDataStore();

// Mobile detection
const { isMobile } = useMobileDetection();

// Default page size: 0 (infinite scroll / view all)
const defaultPageSize = 0;

// Navigate to user creation (used by both header button and mobile search bar)
const navigateToCreateUser = () => {
  router.push('/users/new');
};

// Use the composable for all common functionality
const listManager = useListManagement<User>({
  defaultPageSize,
  itemIdField: 'uuid',
  defaultSortField: 'name',
  defaultSortDirection: 'asc',
  fetchFunction: async (params) => {
    return await dataStore.getPaginatedUsers({
      page: params.page,
      pageSize: params.pageSize,
      sortField: params.sortField,
      sortDirection: params.sortDirection,
      search: params.search,
      role: params.role !== 'all' ? params.role : undefined
    });
  },
  routeBuilder: (user) => `/users/${user.uuid}`,
  mobileSearch: {
    placeholder: 'Search users...',
    createIcon: 'user',
    onCreate: navigateToCreateUser
  }
});

// SSE integration for real-time updates
useListSSE<User>({
  hasItem: listManager.hasItem,
  updateItemField: listManager.updateItemField,
  removeItem: listManager.removeItem,
  prependItem: listManager.prependItem,
  eventTypes: { updated: 'user-updated', created: 'user-created', deleted: 'user-deleted' },
  getEventItemId: (data) => (data.data || data).user_uuid,
  itemKey: 'user'
});

// Define table columns with responsive behavior
// Backend sortable fields: name, role (created_at not supported)
const columns = [
  { field: 'user', label: 'User', width: '1fr', sortable: true, sortKey: 'name', responsive: 'always' as const },
  { field: 'role', label: 'Role', width: 'minmax(100px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'created_at', label: 'Joined', width: 'minmax(140px,auto)', sortable: false, responsive: 'lg' as const }
];

// Build filter options - role is the only available filter from the API
const filterOptions = listManager.buildFilterOptions({
  role: {
    options: [
      { value: 'admin', label: 'Admin' },
      { value: 'technician', label: 'Technician' },
      { value: 'user', label: 'User' }
    ],
    width: 'w-[140px]',
    allLabel: 'All Roles'
  }
});

// Custom grid template for responsive layout (includes checkbox column with auto width)
const gridClass = "grid-cols-[auto_1fr_minmax(100px,auto)] lg:grid-cols-[auto_1fr_minmax(100px,auto)_minmax(140px,auto)]";

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

// Bulk action modal states
const showRoleModal = ref(false);
const bulkActionLoading = ref(false);

// Handle bulk action selection
const handleBulkAction = async (actionId: string) => {
  if (actionId === 'set-role') {
    showRoleModal.value = true;
  } else if (actionId === 'delete') {
    await executeBulkAction('delete');
  }
};

// Execute bulk action
const executeBulkAction = async (action: 'delete' | 'set-role', value?: string) => {
  const ids = listManager.selectedItems.value;
  if (ids.length === 0) return;

  bulkActionLoading.value = true;
  try {
    await userService.bulkAction({ action, ids, value });
    await listManager.refresh();
    listManager.clearSelection();
    showRoleModal.value = false;
  } catch (error) {
    console.error('Bulk action failed:', error);
    alert('Failed to perform bulk action. Please try again.');
  } finally {
    bulkActionLoading.value = false;
  }
};

// Handle bulk role change
const handleBulkRoleChange = (role: string) => {
  executeBulkAction('set-role', role);
};

// Track if currently loading more (to prevent duplicate requests)
const isLoadingMore = ref(false);

// Handle load more from BaseListView's scroll event
const handleLoadMore = async () => {
  if (isLoadingMore.value) return;
  isLoadingMore.value = true;
  try {
    await listManager.loadMore();
  } finally {
    isLoadingMore.value = false;
  }
};

// Expose method for parent (App.vue) to call from header button
defineExpose({
  navigateToCreateUser
});
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
          placeholder="Search users..."
          class="hidden sm:block"
        />

        <!-- Filters -->
        <template v-if="filterOptions.length > 0">
          <div
            v-for="filter in filterOptions"
            :key="filter.name"
            :class="[filter.width || 'w-[120px]']"
          >
            <select
              :value="filter.value"
              @change="e => listManager.handleFilterUpdate(filter.name, (e.target as HTMLSelectElement).value)"
              class="bg-surface-alt border border-default text-primary text-sm rounded-md focus:ring-accent focus:border-accent block w-full py-1 px-2"
            >
              <option
                v-for="option in filter.options"
                :key="option.value"
                :value="option.value"
              >
                {{ option.label }}
              </option>
            </select>
          </div>

          <button
            @click="listManager.resetFilters"
            class="px-2 py-1 text-xs font-medium text-white bg-accent rounded-md hover:opacity-90 focus:ring-2 focus:outline-none focus:ring-accent"
          >
            Reset
          </button>
        </template>

        <!-- Results count -->
        <div class="text-xs text-secondary flex items-center gap-4 ml-auto">
          <span>{{ listManager.totalItems.value }} result{{ listManager.totalItems.value !== 1 ? "s" : "" }}</span>
        </div>
      </div>
    </div>

    <!-- Bulk Actions Bar -->
    <BulkActionsBar
      :selected-count="listManager.selectedItems.value.length"
      :total-count="listManager.totalItems.value"
      :actions="bulkActions"
      item-label="user"
      @action="handleBulkAction"
      @clear-selection="listManager.clearSelection"
      @select-all="listManager.selectAll"
    />

    <!-- Main content -->
    <div class="flex-1 flex flex-col overflow-hidden">
      <BaseListView
        title="Users"
        :is-loading="listManager.loading.value"
        :is-empty="listManager.items.value.length === 0 && !listManager.loading.value"
        :error="listManager.error.value"
        :is-mobile="isMobile"
        empty-icon="users"
        :empty-message="listManager.searchQuery.value ? 'No users match your search' : 'No users found'"
        :empty-description="listManager.searchQuery.value ? 'Try adjusting your search criteria' : 'Invite users to get started'"
        :empty-action-label="!listManager.searchQuery.value ? 'Invite User' : undefined"
        :is-loading-more="isLoadingMore"
        @retry="listManager.fetchItems"
        @empty-action="navigateToCreateUser"
        @load-more="handleLoadMore"
      >
        <!-- Desktop Table View -->
        <template #default>
          <div class="flex-1 overflow-y-auto">
            <DataTable
              :columns="columns"
              :data="listManager.items.value"
              :selected-items="listManager.selectedItems.value"
              :item-id-field="'uuid'"
              :sort-field="listManager.sortField.value"
              :sort-direction="listManager.sortDirection.value"
              :grid-class="gridClass"
              @update:sort="listManager.handleSortUpdate"
              @toggle-selection="listManager.toggleSelection"
              @toggle-all="listManager.toggleAllItems"
              @row-click="listManager.navigateToItem"
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

            <template #cell-created_at="{ value }">
              <DateCell :value="value" format="clean-relative" />
            </template>
            </DataTable>
          </div>
        </template>

        <!-- Mobile/Tablet Card View -->
        <template #mobile-view>
          <div class="flex-1 overflow-y-auto">
            <TransitionGroup
              name="list-stagger"
              tag="div"
              class="flex flex-col"
            >
            <div
              v-for="(user, index) in listManager.items.value"
              :key="user.uuid"
              :style="getStyle(index)"
              @click="listManager.navigateToItem(user)"
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

                <!-- Meta row: email and role -->
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
                </div>
              </div>

              <!-- Chevron -->
              <svg class="w-4 h-4 text-tertiary flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
              </svg>
            </div>
            </TransitionGroup>
          </div>
        </template>
      </BaseListView>
    </div>

    <!-- Pagination Controls (hidden on mobile when using infinite scroll) -->
    <PaginationControls
      v-if="!isMobile"
      :current-page="listManager.currentPage.value"
      :total-pages="listManager.totalPages.value"
      :page-size="listManager.pageSize.value"
      :page-size-options="listManager.pageSizeOptions"
      :show-import="true"
      @update:current-page="listManager.handlePageChange"
      @update:page-size="listManager.handlePageSizeChange"
      @import="() => {}"
    />

    <!-- Bulk Role Modal -->
    <Modal
      :show="showRoleModal"
      title="Set Role"
      size="sm"
      @close="showRoleModal = false"
    >
      <div class="flex flex-col gap-2 p-4">
        <p class="text-sm text-secondary mb-2">
          Update role for {{ listManager.selectedItems.value.length }} user{{ listManager.selectedItems.value.length !== 1 ? 's' : '' }}
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
  </div>
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
