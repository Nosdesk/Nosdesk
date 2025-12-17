<script setup lang="ts">
import { formatDate as formatDateUtil, formatDateTime } from '@/utils/dateUtils';
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
    return formatDateTime(dateString);
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
    <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
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
            class="px-2 py-1 text-xs font-medium text-white bg-brand-blue rounded-md hover:opacity-90 focus:ring-2 focus:outline-none focus:ring-brand-blue"
          >
            Reset
          </button>
        </template>

        <div class="text-xs text-secondary ml-auto">
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
              <span v-else class="text-xs text-tertiary">Unassigned</span>
            </template>
            
            <template #cell-warranty_status="{ value }">
              <StatusBadgeCell type="warranty" :value="value" />
            </template>
          </DataTable>
        </template>

        <!-- Mobile/Tablet Card View -->
        <template #mobile-view>
          <div class="flex flex-col divide-y divide-default">
            <div
              v-for="device in listManager.items.value"
              :key="device.id"
              @click="listManager.navigateToItem(device)"
              class="flex items-center gap-3 px-3 py-2.5 hover:bg-surface-hover active:bg-surface-alt transition-colors cursor-pointer"
            >
              <!-- Device icon -->
              <div class="w-10 h-10 rounded-lg bg-surface-alt flex items-center justify-center flex-shrink-0">
                <svg class="w-5 h-5 text-secondary" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                </svg>
              </div>

              <!-- Main content -->
              <div class="flex-1 min-w-0">
                <!-- Name and ID -->
                <div class="flex items-center gap-2">
                  <span class="text-xs text-secondary font-medium flex-shrink-0">#{{ device.id }}</span>
                  <span class="text-sm text-primary font-medium truncate">{{ device.name }}</span>
                </div>

                <!-- Meta row: manufacturer, model, serial, warranty, user - responsive layout -->
                <div class="flex flex-wrap items-center gap-2 mt-1.5 text-xs">
                  <!-- Manufacturer & Model -->
                  <span class="text-secondary flex-shrink-0">{{ device.manufacturer || 'Unknown' }} {{ device.model }}</span>

                  <!-- Serial Number -->
                  <span class="text-tertiary font-mono flex-shrink-0">SN: {{ device.serial_number }}</span>

                  <!-- Warranty Status -->
                  <span
                    class="inline-flex items-center px-1.5 py-0.5 rounded font-medium border flex-shrink-0"
                    :class="{
                      'bg-status-success-muted text-status-success border-status-success/30': device.warranty_status === 'Active',
                      'bg-status-warning-muted text-status-warning border-status-warning/30': device.warranty_status === 'Warning',
                      'bg-status-error-muted text-status-error border-status-error/30': device.warranty_status === 'Expired',
                      'bg-surface-alt text-secondary border-default': device.warranty_status === 'Unknown'
                    }"
                  >
                    {{ device.warranty_status }}
                  </span>

                  <!-- Primary User -->
                  <div class="flex items-center gap-1 min-w-0 flex-shrink-0">
                    <span class="text-tertiary">User:</span>
                    <template v-if="device.primary_user">
                      <div class="[&>div]:!w-4 [&>div]:!h-4 [&>div>*]:!w-4 [&>div>*]:!h-4 [&>div>*]:!text-[8px]">
                        <UserAvatar
                          :name="device.primary_user.uuid"
                          :user-name="device.primary_user.name"
                          :avatar="device.primary_user.avatar_thumb || device.primary_user.avatar_url"
                          :show-name="false"
                          :clickable="false"
                          size="xs"
                        />
                      </div>
                      <span class="text-secondary truncate max-w-[100px]">{{ device.primary_user.name }}</span>
                    </template>
                    <span v-else class="text-tertiary italic">Unassigned</span>
                  </div>
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
  background: var(--bg-app);
}

.overflow-y-auto::-webkit-scrollbar-thumb,
.overflow-x-auto::-webkit-scrollbar-thumb {
  background: var(--border-default);
  border-radius: 4px;
}

.overflow-y-auto::-webkit-scrollbar-thumb:hover,
.overflow-x-auto::-webkit-scrollbar-thumb:hover {
  background: var(--border-strong);
}

.overflow-x-auto::-webkit-scrollbar-corner {
  background: var(--bg-app);
}
</style> 