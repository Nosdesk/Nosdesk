<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import BaseListView from '@/components/common/BaseListView.vue'
import DataTable from '@/components/common/DataTable.vue'
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue'
import PaginationControls from '@/components/common/PaginationControls.vue'

import { IdCell, TextCell, StatusBadgeCell, UserAvatarCell } from '@/components/common/cells'
import UserAvatar from '@/components/UserAvatar.vue'
import { useListManagement } from '@/composables/useListManagement'
import { useDataStore } from '@/stores/dataStore'
import { getPaginatedDevices } from '@/services/deviceService'
import type { Device } from '@/types/device'

const router = useRouter()
const dataStore = useDataStore()

// Use the composable for all common functionality
const listManager = useListManagement<Device>({
  itemIdField: 'id',
  defaultSortField: 'name',
  defaultSortDirection: 'asc',
  fetchFunction: async (params) => {
    const response = await getPaginatedDevices({
      page: params.page,
      pageSize: params.pageSize,
      sortField: params.sortField,
      sortDirection: params.sortDirection,
      search: params.search,
      type: params.type,
      warranty: params.warranty
    });
    
    // Pre-warm user cache for efficient avatar loading
    preWarmUserCache(response.data);
    
    return response;
  },
  routeBuilder: (device) => `/devices/${device.id}`
});

// Pre-warm user cache with all primary users from devices
const preWarmUserCache = async (devices: Device[]) => {
  try {
    // Extract unique user UUIDs from devices
    const userUuids = [...new Set(
      devices
        .map(device => device.primary_user?.uuid)
        .filter((uuid): uuid is string => uuid !== undefined && uuid.length === 36) // Valid UUIDs only
    )];
    
    if (userUuids.length === 0) return;
    
    console.log(`🚀 Pre-warming user cache for ${userUuids.length} device users`);
    
    // Check which users are already cached to avoid unnecessary requests
    const uncachedUuids = userUuids.filter(uuid => {
      const cachedName = dataStore.getUserName(uuid);
      return !cachedName; // If no cached name, user is not cached
    });
    
    if (uncachedUuids.length === 0) {
      console.log('✅ All device users already cached, skipping pre-warm');
      return;
    }
    
    console.log(`📡 Loading ${uncachedUuids.length} uncached users (${userUuids.length - uncachedUuids.length} already cached)`);
    
    // Use our batching system to efficiently load only uncached users
    await dataStore.getUsersByUuids(uncachedUuids);
    
    console.log('✅ Device user cache pre-warming completed');
  } catch (error) {
    console.warn('Failed to pre-warm user cache:', error);
    // Don't throw - this is an optimization, not critical
  }
};

// Define table columns with responsive behavior
const columns = [
  { field: 'id', label: 'ID', width: 'minmax(60px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'name', label: 'Device Name', width: '1fr', sortable: true, responsive: 'always' as const },
  { field: 'serial_number', label: 'Serial Number', width: 'minmax(120px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'manufacturer', label: 'Manufacturer', width: 'minmax(120px,auto)', sortable: true, responsive: 'lg' as const },
  { field: 'model', label: 'Model', width: 'minmax(120px,auto)', sortable: true, responsive: 'lg' as const },
  { field: 'primary_user', label: 'Primary User', width: 'minmax(120px,auto)', sortable: false, responsive: 'md' as const },
  { field: 'warranty_status', label: 'Warranty', width: 'minmax(100px,auto)', sortable: true, responsive: 'always' as const }
];

// Get available filter options
const availableManufacturers = computed(() => {
  return ['Microsoft Corporation', 'Dell Inc.', 'HP Inc.', 'Lenovo', 'Apple Inc.', 'ASUS', 'Acer'];
});

const availableWarrantyStatuses = computed(() => {
  return ['Active', 'Warning', 'Expired', 'Unknown'];
});

// Build filter options
const filterOptions = computed(() => {
  return listManager.buildFilterOptions({
    type: {
      options: availableManufacturers.value.map(manufacturer => ({ 
        value: manufacturer.toLowerCase(), 
        label: manufacturer 
      })),
      width: 'w-[160px]',
      allLabel: 'All Manufacturers'
    },
    warranty: {
      options: availableWarrantyStatuses.value.map(status => ({ 
        value: status.toLowerCase(), 
        label: status 
      })),
      width: 'w-[120px]',
      allLabel: 'All Warranties'
    }
  });
});

// Custom grid template for responsive layout
const gridClass = "grid-cols-[auto_1fr_minmax(100px,auto)] md:grid-cols-[auto_minmax(60px,auto)_1fr_minmax(120px,auto)_minmax(120px,auto)_minmax(100px,auto)] lg:grid-cols-[auto_minmax(60px,auto)_1fr_minmax(120px,auto)_minmax(120px,auto)_minmax(120px,auto)_minmax(120px,auto)_minmax(100px,auto)]";



// Navigate to create device
const navigateToCreateDevice = () => {
  router.push('/devices/new');
};

// Format date function
const formatDate = (dateString: string) => {
  try {
    const date = new Date(dateString);
    if (isNaN(date.getTime())) return 'Unknown';
    return date.toLocaleString();
  } catch (error) {
    console.error("Error formatting date:", error);
    return 'Unknown';
  }
};

// Expose method for parent (App.vue) to call from header button
defineExpose({
  navigateToCreateDevice
});
</script>

<template>
  <div class="flex flex-col h-full overflow-hidden">
    <!-- Search and filter bar -->
    <div class="sticky top-0 z-20 bg-slate-800 border-b border-slate-700 shadow-md">
      <div class="p-2 flex items-center gap-2 flex-wrap">
        <DebouncedSearchInput
          v-model="listManager.searchQuery.value"
          placeholder="Search devices..."
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
              class="bg-slate-700 border border-slate-600 text-white text-sm rounded-md focus:ring-blue-500 focus:border-blue-500 block w-full py-1 px-2"
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
            class="px-2 py-1 text-xs font-medium text-white bg-blue-600 rounded-md hover:bg-blue-700 focus:ring-2 focus:outline-none focus:ring-blue-800"
          >
            Reset
          </button>
        </template>

        <div class="text-xs text-slate-400 ml-auto">
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
            
            <template #cell-name="{ value }">
              <TextCell :value="value" font-weight="medium" />
            </template>
            
            <template #cell-serial_number="{ value }">
              <TextCell :value="value" />
            </template>
            
            <template #cell-manufacturer="{ value }">
              <TextCell :value="value || 'Unknown'" />
            </template>
            
            <template #cell-model="{ value }">
              <TextCell :value="value" />
            </template>
            
            <template #cell-primary_user="{ item }">
              <UserAvatarCell 
                v-if="item.primary_user"
                :user-id="item.primary_user.uuid" 
                :user-name="item.primary_user.name"
                :avatar="item.primary_user.avatar_thumb || item.primary_user.avatar_url"
                :show-name="true" 
              />
              <span v-else class="text-xs text-slate-500">Unassigned</span>
            </template>
            
            <template #cell-warranty_status="{ value }">
              <StatusBadgeCell type="warranty" :value="value" />
            </template>
          </DataTable>
        </template>

        <!-- Mobile Card View -->
        <template #mobile-view>
          <div class="flex flex-col gap-2 p-2">
            <div
              v-for="device in listManager.items.value"
              :key="device.id"
              @click="listManager.navigateToItem(device)"
              class="bg-slate-800 rounded-lg p-3 hover:bg-slate-700/50 transition-colors cursor-pointer"
            >
              <div class="flex items-center gap-3">
                <div class="flex-grow-0">
                  <input
                    type="checkbox"
                    class="w-4 h-4 rounded border-slate-600 bg-slate-700 text-blue-600 focus:ring-blue-500"
                    :checked="listManager.selectedItems.value.includes(device.id.toString())"
                    @click.stop="(event) => listManager.toggleSelection(event, device.id.toString())"
                  />
                </div>
                <div class="flex-1 min-w-0">
                  <div class="flex items-center justify-between mb-2">
                    <div class="font-medium truncate text-slate-200">{{ device.name }}</div>
                    <div class="text-xs text-slate-400 ml-2">#{{ device.id }}</div>
                  </div>
                  <div class="flex flex-wrap gap-1 mb-2">
                    <span class="bg-slate-700/50 px-2 py-1 rounded text-xs text-slate-300">
                      {{ device.manufacturer || 'Unknown' }}
                    </span>
                    <span class="bg-slate-700/50 px-2 py-1 rounded text-xs text-slate-300">
                      {{ device.model }}
                    </span>
                    <span class="bg-slate-700/50 px-2 py-1 rounded text-xs text-slate-300 font-mono">
                      SN: {{ device.serial_number }}
                    </span>
                    <span 
                      class="px-2 py-1 rounded text-xs"
                      :class="{
                        'bg-green-900/30 text-green-400': device.warranty_status === 'Active',
                        'bg-yellow-900/30 text-yellow-400': device.warranty_status === 'Warning',
                        'bg-red-900/30 text-red-400': device.warranty_status === 'Expired',
                        'bg-slate-700 text-slate-400': device.warranty_status === 'Unknown'
                      }"
                    >
                      {{ device.warranty_status }}
                    </span>
                  </div>
                  <div class="flex items-center justify-between text-xs">
                    <div v-if="device.primary_user" class="flex items-center gap-2">
                      <span class="text-slate-400">Assigned to:</span>
                      <UserAvatar 
                        :name="device.primary_user.uuid" 
                        :user-name="device.primary_user.name"
                        :avatar="device.primary_user.avatar_thumb || device.primary_user.avatar_url"
                        :show-name="true" 
                        size="xs" 
                      />
                    </div>
                    <div v-else class="text-slate-500">Unassigned</div>
                    <div class="text-slate-400">
                      {{ device.updated_at ? formatDate(device.updated_at) : 'Never' }}
                    </div>
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