<script setup lang="ts">
import { computed, ref } from "vue";
import { useRouter } from "vue-router";
import BaseListView from "@/components/common/BaseListView.vue";
import DataTable from "@/components/common/DataTable.vue";
import DebouncedSearchInput from "@/components/common/DebouncedSearchInput.vue";
import PaginationControls from "@/components/common/PaginationControls.vue";
import { IdCell, TextCell, StatusBadgeCell, UserInfoCell } from "@/components/common/cells";
import UserAvatar from "@/components/UserAvatar.vue";
import { useListManagement } from "@/composables/useListManagement";
import { useDataStore } from "@/stores/dataStore";
import { useOptimisticUpdates } from "@/composables/useOptimisticUpdates";
import type { User } from "@/services/userService";

// Extended user interface for UI display
interface UIUser extends User {
  department?: string;
}

const router = useRouter();
const dataStore = useDataStore();
const optimisticUpdates = useOptimisticUpdates();

// Use the composable for all common functionality
const listManager = useListManagement<UIUser>({
  itemIdField: 'uuid',
  defaultSortField: 'name',
  defaultSortDirection: 'asc',
  fetchFunction: async (params) => {
    const response = await dataStore.getPaginatedUsers({
      page: params.page,
      pageSize: params.pageSize,
      sortField: params.sortField,
      sortDirection: params.sortDirection,
      search: params.search,
      role: params.role !== 'all' ? params.role : undefined
    });

    // Transform backend users to UI users with additional properties
    const transformedData = response.data.map(user => ({
      ...user,
      department: "IT Support", // Default department (could be added to backend later)
    }));

    return {
      data: transformedData,
      total: response.total,
      totalPages: response.totalPages
    };
  },
  routeBuilder: (user) => `/users/${user.uuid}`
});

// Define table columns with responsive behavior
const columns = [
  { field: 'user', label: 'User', width: '1fr', sortable: false, responsive: 'always' as const },
  { field: 'role', label: 'Role', width: 'minmax(120px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'department', label: 'Department', width: 'minmax(120px,auto)', sortable: false, responsive: 'lg' as const }
];

// Get available filter options
const availableRoles = computed(() => {
  return ['admin', 'user', 'technician'];
});

// Build filter options
const filterOptions = computed(() => {
  return listManager.buildFilterOptions({
    role: {
      options: availableRoles.value.map(role => ({ 
        value: role.toLowerCase(), 
        label: role.charAt(0).toUpperCase() + role.slice(1)
      })),
      width: 'w-[150px]',
      allLabel: 'All Roles'
    }
  });
});

// Custom grid template for responsive layout
const gridClass = "grid-cols-[auto_1fr_minmax(120px,auto)] md:grid-cols-[auto_1fr_minmax(120px,auto)] lg:grid-cols-[auto_1fr_minmax(120px,auto)_minmax(120px,auto)]";

// Navigate to user creation
const navigateToCreateUser = () => {
  router.push('/users/new');
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
        <DebouncedSearchInput
          v-model="listManager.searchQuery.value"
          placeholder="Search users..."
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

    <!-- Main content -->
    <div class="flex-1 flex flex-col overflow-hidden">
      <BaseListView
        title="Users"
        :is-loading="listManager.loading.value"
        :is-empty="listManager.items.value.length === 0 && !listManager.loading.value"
        :error="listManager.error.value"
        empty-icon="users"
        :empty-message="listManager.searchQuery.value ? 'No users match your search' : 'No users found'"
        :empty-description="listManager.searchQuery.value ? 'Try adjusting your search criteria' : 'Invite users to get started'"
        :empty-action-label="!listManager.searchQuery.value ? 'Invite User' : undefined"
        :results-count="listManager.totalItems.value"
        :sort-field="listManager.sortField.value"
        :sort-direction="listManager.sortDirection.value"
        :columns="[]"
        :selected-items="listManager.selectedItems.value"
        :visible-items="listManager.items.value"
        :item-id-field="'uuid'"
        :enable-selection="false"
        @retry="listManager.fetchItems"
        @empty-action="navigateToCreateUser"
      >
        <!-- Desktop Table View -->
        <template #default>
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
                :user-email="item.email"
                :show-avatar="true"
                :show-name="true"
                :show-email="true"
              />
            </template>
            
            <template #cell-role="{ value }">
              <StatusBadgeCell type="role" :value="value" />
            </template>
            
            <template #cell-department="{ value }">
              <TextCell :value="value || 'N/A'" />
            </template>
          </DataTable>
        </template>

        <!-- Mobile/Tablet Card View -->
        <template #mobile-view>
          <div class="flex flex-col divide-y divide-default">
            <div
              v-for="user in listManager.items.value"
              :key="user.uuid"
              @click="listManager.navigateToItem(user)"
              class="flex items-center gap-3 px-3 py-2.5 hover:bg-surface-hover active:bg-surface-alt transition-colors cursor-pointer"
            >
              <!-- Avatar -->
              <UserAvatar
                :name="user.uuid"
                :userName="user.name"
                size="sm"
                :clickable="false"
                :show-name="false"
                class="flex-shrink-0"
              />

              <!-- Main content -->
              <div class="flex-1 min-w-0">
                <!-- Name -->
                <div class="text-sm text-primary font-medium truncate">{{ user.name }}</div>

                <!-- Meta row: email, role, department - responsive layout -->
                <div class="flex flex-wrap items-center gap-2 mt-1 text-xs">
                  <span v-if="user.email" class="text-tertiary truncate max-w-[200px]">{{ user.email }}</span>
                  <span
                    class="inline-flex items-center px-1.5 py-0.5 rounded font-medium capitalize"
                    :class="{
                      'bg-status-error/20 text-status-error': user.role === 'admin',
                      'bg-accent/20 text-accent': user.role === 'technician',
                      'bg-surface-alt text-secondary': user.role === 'user'
                    }"
                  >
                    {{ user.role }}
                  </span>
                  <span class="text-secondary">{{ user.department || 'N/A' }}</span>
                </div>
              </div>

              <!-- Chevron -->
              <svg class="w-4 h-4 text-tertiary flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
              </svg>
            </div>
          </div>
        </template>
      </BaseListView>
    </div>

    <!-- Pagination Controls -->
    <PaginationControls
      :current-page="listManager.currentPage.value"
      :total-pages="listManager.totalPages.value"
      :page-size="listManager.pageSize.value"
      :page-size-options="listManager.pageSizeOptions"
      :show-import="true"
      @update:current-page="listManager.handlePageChange"
      @update:page-size="listManager.handlePageSizeChange"
      @import="() => {}"
    />

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
  background: var(--color-bg-app);
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
  background: var(--color-bg-app);
}
</style>
