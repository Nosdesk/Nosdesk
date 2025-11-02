// views/TicketsListView.vue
<script setup lang="ts">
import ticketService from "@/services/ticketService";
import { computed, onMounted, watch, nextTick } from "vue";
import { useRoute, useRouter } from "vue-router";
import BaseListView from "@/components/common/BaseListView.vue";
import DataTable from "@/components/common/DataTable.vue";
import DebouncedSearchInput from "@/components/common/DebouncedSearchInput.vue";
import PaginationControls from "@/components/common/PaginationControls.vue";
import { IdCell, TextCell, StatusBadgeCell, UserAvatarCell, DateCell } from "@/components/common/cells";
import StatusBadge from "@/components/StatusBadge.vue";
import UserAvatar from "@/components/UserAvatar.vue";
import { useListManagement } from "@/composables/useListManagement";
import type { Ticket } from "@/services/ticketService";

const route = useRoute();
const router = useRouter();

// Extract URL params first
const urlParams = route.query;
const initialFilters: Record<string, string> = {};

// Set initial values from URL
if (urlParams.status && typeof urlParams.status === 'string') {
  initialFilters.status = urlParams.status;
}
if (urlParams.priority && typeof urlParams.priority === 'string') {
  initialFilters.priority = urlParams.priority;
}
if (urlParams.createdOn && typeof urlParams.createdOn === 'string') {
  initialFilters.createdOn = urlParams.createdOn;
}
if (urlParams.createdAfter && typeof urlParams.createdAfter === 'string') {
  initialFilters.createdAfter = urlParams.createdAfter;
}
if (urlParams.createdBefore && typeof urlParams.createdBefore === 'string') {
  initialFilters.createdBefore = urlParams.createdBefore;
}
if (urlParams.modifiedOn && typeof urlParams.modifiedOn === 'string') {
  initialFilters.modifiedOn = urlParams.modifiedOn;
}
if (urlParams.modifiedAfter && typeof urlParams.modifiedAfter === 'string') {
  initialFilters.modifiedAfter = urlParams.modifiedAfter;
}
if (urlParams.modifiedBefore && typeof urlParams.modifiedBefore === 'string') {
  initialFilters.modifiedBefore = urlParams.modifiedBefore;
}
if (urlParams.closedOn && typeof urlParams.closedOn === 'string') {
  initialFilters.closedOn = urlParams.closedOn;
}
if (urlParams.closedAfter && typeof urlParams.closedAfter === 'string') {
  initialFilters.closedAfter = urlParams.closedAfter;
}
if (urlParams.closedBefore && typeof urlParams.closedBefore === 'string') {
  initialFilters.closedBefore = urlParams.closedBefore;
}

const initialSearchQuery = (urlParams.search && typeof urlParams.search === 'string') ? urlParams.search : '';
const initialPage = (urlParams.page && typeof urlParams.page === 'string') ? parseInt(urlParams.page) : 1;
const initialPageSize = (urlParams.pageSize && typeof urlParams.pageSize === 'string') ? parseInt(urlParams.pageSize) : 25;
const initialSortField = (urlParams.sortField && typeof urlParams.sortField === 'string') ? urlParams.sortField : 'id';
const initialSortDirection = (urlParams.sortDirection && typeof urlParams.sortDirection === 'string') ? urlParams.sortDirection as 'asc' | 'desc' : 'asc';

// Use the composable with initial values
const listManager = useListManagement<Ticket>({
  itemIdField: 'id',
  defaultSortField: initialSortField,
  defaultSortDirection: initialSortDirection,
  fetchFunction: async (params) => {
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
    }, 'paginated-tickets');
  },
  routeBuilder: (ticket) => `/tickets/${ticket.id}`
});

// Set initial values
listManager.searchQuery.value = initialSearchQuery;
listManager.filters.value = initialFilters;
listManager.currentPage.value = initialPage;
listManager.pageSize.value = initialPageSize;

// Update URL when filters change
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
    
    // Add search to URL
    if (listManager.searchQuery.value) {
      query.search = listManager.searchQuery.value;
    }
    
    // Add filters to URL
    if (listManager.filters.value.status && listManager.filters.value.status !== 'all') {
      query.status = listManager.filters.value.status;
    }
    
    if (listManager.filters.value.priority && listManager.filters.value.priority !== 'all') {
      query.priority = listManager.filters.value.priority;
    }
    
    // Add date filters to URL
    if (listManager.filters.value.createdOn) {
      query.createdOn = listManager.filters.value.createdOn;
    }
    
    if (listManager.filters.value.createdAfter) {
      query.createdAfter = listManager.filters.value.createdAfter;
    }
    
    if (listManager.filters.value.createdBefore) {
      query.createdBefore = listManager.filters.value.createdBefore;
    }
    
    if (listManager.filters.value.modifiedOn) {
      query.modifiedOn = listManager.filters.value.modifiedOn;
    }
    
    if (listManager.filters.value.modifiedAfter) {
      query.modifiedAfter = listManager.filters.value.modifiedAfter;
    }
    
    if (listManager.filters.value.modifiedBefore) {
      query.modifiedBefore = listManager.filters.value.modifiedBefore;
    }
    
    if (listManager.filters.value.closedOn) {
      query.closedOn = listManager.filters.value.closedOn;
    }
    
    if (listManager.filters.value.closedAfter) {
      query.closedAfter = listManager.filters.value.closedAfter;
    }
    
    if (listManager.filters.value.closedBefore) {
      query.closedBefore = listManager.filters.value.closedBefore;
    }
    
    // Add pagination to URL (only if not default)
    if (listManager.currentPage.value > 1) {
      query.page = listManager.currentPage.value.toString();
    }
    
    if (listManager.pageSize.value !== 10) {
      query.pageSize = listManager.pageSize.value.toString();
    }
    
    // Add sorting to URL (only if not default)
    if (listManager.sortField.value !== 'id') {
      query.sortField = listManager.sortField.value;
    }
    
    if (listManager.sortDirection.value !== 'asc') {
      query.sortDirection = listManager.sortDirection.value;
    }
    
    // Update URL without triggering navigation
    router.replace({ 
      path: route.path, 
      query: Object.keys(query).length > 0 ? query : undefined 
    });
  },
  { deep: true }
);

// Define table columns with responsive behavior
const columns = [
  { field: 'id', label: 'ID', width: 'minmax(60px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'title', label: 'Title', width: '1fr', sortable: true, responsive: 'always' as const },
  { field: 'status', label: 'Status', width: 'minmax(100px,auto)', sortable: true, responsive: 'always' as const },
  { field: 'priority', label: 'Priority', width: 'minmax(100px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'created', label: 'Created', width: 'minmax(120px,auto)', sortable: true, responsive: 'lg' as const },
  { field: 'requester', label: 'Requester', width: 'minmax(120px,auto)', sortable: true, responsive: 'lg' as const },
  { field: 'assignee', label: 'Assignee', width: 'minmax(120px,auto)', sortable: true, responsive: 'lg' as const }
];

// Get available filter options
const availableStatuses = computed(() => {
  if (!listManager.items.value.length) return [];
  const statuses = new Set(listManager.items.value.map((ticket) => ticket.status));
  return Array.from(statuses) as string[];
});

const availablePriorities = computed(() => {
  if (!listManager.items.value.length) return [];
  const priorities = new Set(listManager.items.value.map((ticket) => ticket.priority));
  return Array.from(priorities) as string[];
});

// Build filter options
const filterOptions = computed(() => {
  return listManager.buildFilterOptions({
    status: {
      options: availableStatuses.value.map(status => ({ 
        value: status.toLowerCase(), 
        label: status.charAt(0).toUpperCase() + status.slice(1) 
      })),
      width: 'w-[120px]',
      allLabel: 'All Statuses'
    },
    priority: {
      options: availablePriorities.value.map(priority => ({ 
        value: priority.toLowerCase(), 
        label: priority.charAt(0).toUpperCase() + priority.slice(1) 
      })),
      width: 'w-[120px]',
      allLabel: 'All Priorities'
    }
  });
});

// Custom grid template for responsive layout
const gridClass = "grid-cols-[auto_1fr_minmax(80px,auto)] md:grid-cols-[auto_minmax(60px,auto)_1fr_minmax(80px,auto)_minmax(80px,auto)] lg:grid-cols-[auto_minmax(60px,auto)_1fr_minmax(100px,auto)_minmax(100px,auto)_minmax(120px,auto)_minmax(120px,auto)_minmax(120px,auto)]";
</script>

<template>
  <div class="flex flex-col h-full overflow-hidden">
    <!-- Search and filter bar -->
    <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
      <div class="p-2 flex items-center gap-2 flex-wrap">
        <DebouncedSearchInput
          v-model="listManager.searchQuery.value"
          placeholder="Search tickets..."
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
              class="bg-surface-alt border border-default text-primary text-sm rounded-md focus:ring-brand-blue focus:border-brand-blue block w-full py-1 px-2"
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
            class="px-2 py-1 text-xs font-medium text-primary bg-brand-blue rounded-md hover:bg-brand-blue/80 focus:ring-2 focus:outline-none focus:ring-brand-blue/50"
          >
            Reset
          </button>
        </template>

        <div class="text-xs text-tertiary ml-auto">
          {{ listManager.totalItems.value }} result{{ listManager.totalItems.value !== 1 ? "s" : "" }}
        </div>
      </div>
    </div>

    <!-- Main content -->
    <div class="flex-1 flex flex-col overflow-hidden">
      <BaseListView
        title=""
        :search-query="''"
        :is-loading="listManager.loading.value"
        :is-empty="listManager.items.value.length === 0 && !listManager.loading.value"
        :error="listManager.error.value"
        :filters="[]"
        :results-count="listManager.totalItems.value"
        :selected-items="listManager.selectedItems.value"
        :visible-items="listManager.items.value"
        :item-id-field="'id'"
        :enable-selection="false"
        :sort-field="listManager.sortField.value"
        :sort-direction="listManager.sortDirection.value"
        :columns="[]"
        :show-add-button="false"
        @retry="listManager.fetchItems"
      >
        <!-- Desktop Table View -->
        <template #default>
          <DataTable
            :columns="columns"
            :data="listManager.items.value"
            :selected-items="listManager.selectedItems.value"
            :sort-field="listManager.sortField.value"
            :sort-direction="listManager.sortDirection.value"
            :grid-class="gridClass"
            @update:sort="listManager.handleSortUpdate"
            @toggle-selection="listManager.toggleSelection"
            @toggle-all="listManager.toggleAllItems"
            @row-click="listManager.navigateToItem"
          >
            <!-- Custom cell templates -->
            <template #cell-id="{ value }">
              <IdCell :id="value" />
            </template>
            
            <template #cell-title="{ value }">
              <TextCell :value="value" font-weight="medium" />
            </template>
            
            <template #cell-status="{ value }">
              <StatusBadge type="status" :value="value" :short="true" />
            </template>
            
            <template #cell-priority="{ value }">
              <StatusBadge type="priority" :value="value" :short="true" />
            </template>
            
            <template #cell-created="{ value }">
              <DateCell :value="value" />
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

        <!-- Mobile Card View -->
        <template #mobile-view>
          <div class="flex flex-col gap-2 p-2">
            <div
              v-for="ticket in listManager.items.value"
              :key="ticket.id"
              @click="listManager.navigateToItem(ticket)"
              class="bg-surface-alt rounded-lg p-3 hover:bg-surface-hover transition-colors cursor-pointer"
            >
              <div class="flex items-start gap-3">
                <div class="flex-grow-0">
                  <input
                    type="checkbox"
                    class="w-4 h-4 rounded border-default bg-surface-alt text-brand-blue focus:ring-brand-blue"
                    :checked="listManager.selectedItems.value.includes(ticket.id.toString())"
                    @click.stop="(event) => listManager.toggleSelection(event, ticket.id.toString())"
                  />
                </div>
                <div class="flex-1 min-w-0">
                  <div class="flex items-center justify-between">
                    <div class="font-medium truncate text-primary">{{ ticket.title }}</div>
                    <div class="text-xs text-tertiary ml-2">#{{ ticket.id }}</div>
                  </div>
                  <div class="mt-2 flex flex-wrap gap-2 text-xs">
                    <StatusBadge type="status" :value="ticket.status" :short="true" :compact="true" />
                    <StatusBadge type="priority" :value="ticket.priority" :short="true" :compact="true" />
                  </div>
                  <div class="mt-2 flex items-center gap-2 text-xs">
                    <div class="flex items-center gap-1">
                      <UserAvatar 
                        v-if="ticket.requester_user || ticket.requester" 
                        :name="ticket.requester_user?.name || ticket.requester" 
                        :avatarUrl="ticket.requester_user?.avatar_thumb" 
                        :userUuid="ticket.requester_user?.uuid"
                        size="xs" 
                      />
                      <span v-else class="text-tertiary">No requester</span>
                      <span class="text-secondary">Requester</span>
                    </div>
                    <div class="flex items-center gap-1">
                      <UserAvatar
                        v-if="ticket.assignee_user || ticket.assignee"
                        :name="ticket.assignee_user?.name || ticket.assignee"
                        :avatarUrl="ticket.assignee_user?.avatar_thumb"
                        :userUuid="ticket.assignee_user?.uuid"
                        size="xs"
                      />
                      <span v-else class="text-tertiary">Unassigned</span>
                      <span class="text-secondary">Assignee</span>
                    </div>
                  </div>
                  <div class="mt-2 text-xs text-secondary">
                    Created: {{ listManager.formatDate(ticket.created) }}
                  </div>
                </div>
              </div>
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
  background: #0f172a; /* slate-900 */
}

.overflow-y-auto::-webkit-scrollbar-thumb,
.overflow-x-auto::-webkit-scrollbar-thumb {
  background: #475569; /* slate-600 */
  border-radius: 4px;
}

.overflow-y-auto::-webkit-scrollbar-thumb:hover,
.overflow-x-auto::-webkit-scrollbar-thumb:hover {
  background: #64748b; /* slate-500 */
}

.overflow-x-auto::-webkit-scrollbar-corner {
  background: #0f172a; /* slate-900 */
}
</style>
