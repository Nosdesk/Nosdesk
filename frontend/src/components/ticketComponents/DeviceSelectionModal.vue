<!-- components/ticketComponents/DeviceSelectionModal.vue -->
<script setup lang="ts">
import { ref, watch, computed, nextTick } from 'vue';
import { useRouter } from 'vue-router';
import Modal from '@/components/Modal.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import { getPaginatedDevices } from '@/services/deviceService';
import { getUserDevices, getPaginatedDevicesExcluding } from '@/services/deviceService';
import type { Device } from '@/types/device';

const router = useRouter();
const props = defineProps<{
  show: boolean;
  currentTicketId?: number;
  existingDeviceIds?: number[];
  requesterUuid?: string; // Add requester UUID for prioritization
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'select-device', device: Device): void;
}>();

// State management
const searchQuery = ref('');
const devices = ref<Device[]>([]);
const requesterDevices = ref<Device[]>([]);
const loading = ref(false);
const loadingMore = ref(false);
const loadingRequesterDevices = ref(false);
const error = ref<string | null>(null);
const hasMore = ref(false);
const currentPage = ref(1);
const totalDevices = ref(0);
const pageSize = 20; // Load devices in chunks of 20

// Search debouncing
let searchTimeout: ReturnType<typeof setTimeout> | null = null;
const searchDebounceMs = 300;

// Scroll container reference
const scrollContainer = ref<HTMLElement | null>(null);

// Load requester's devices first (for immediate display at top)
const loadRequesterDevices = async () => {
  if (import.meta.env.DEV) {
    console.log('DeviceSelectionModal: loadRequesterDevices called with requesterUuid:', props.requesterUuid);
  }
  
  if (!props.requesterUuid) {
    if (import.meta.env.DEV) {
      console.log('DeviceSelectionModal: No requester UUID provided, skipping requester devices load');
    }
    return;
  }
  
  loadingRequesterDevices.value = true;
  try {
    if (import.meta.env.DEV) {
      console.log(`DeviceSelectionModal: Fetching devices for requester ${props.requesterUuid}`);
    }
    
    const devices = await getUserDevices(props.requesterUuid);
    
    if (import.meta.env.DEV) {
      console.log(`DeviceSelectionModal: Received ${devices.length} devices from getUserDevices:`, devices);
    }
    
    // Filter out already assigned devices
    let filteredDevices = devices;
    if (props.existingDeviceIds && props.existingDeviceIds.length > 0) {
      filteredDevices = devices.filter(device => 
        !props.existingDeviceIds?.includes(device.id)
      );
      
      if (import.meta.env.DEV) {
        console.log(`DeviceSelectionModal: Filtered out existing devices. ${devices.length} -> ${filteredDevices.length} devices`);
        console.log('DeviceSelectionModal: Existing device IDs:', props.existingDeviceIds);
      }
    }
    
    requesterDevices.value = filteredDevices;
    
    if (import.meta.env.DEV) {
      console.log(`DeviceSelectionModal: Set requesterDevices to ${filteredDevices.length} devices:`, filteredDevices);
    }
  } catch (err) {
    console.error('DeviceSelectionModal: Error loading requester devices:', err);
    // Don't show error for this as it's optional
    requesterDevices.value = [];
  } finally {
    loadingRequesterDevices.value = false;
  }
};

// Get IDs of devices to exclude from pagination (requester's devices + existing)
const getExcludeIds = (): number[] => {
  const excludeIds = [...(props.existingDeviceIds || [])];
  
  // Add requester device IDs to exclude list
  requesterDevices.value.forEach(device => {
    if (!excludeIds.includes(device.id)) {
      excludeIds.push(device.id);
    }
  });
  
  return excludeIds;
};

// Load devices with pagination and search (excluding requester's devices and existing)
const loadDevices = async (page: number = 1, search: string = '', append: boolean = false) => {
  if (page === 1) {
    loading.value = true;
  } else {
    loadingMore.value = true;
  }
  error.value = null;
  
  try {
    const excludeIds = getExcludeIds();
    
    const response = await getPaginatedDevicesExcluding({
      page,
      pageSize,
      search: search.trim() || undefined,
      excludeIds: excludeIds.length > 0 ? excludeIds : undefined
    });
    
    if (append && page > 1) {
      devices.value = [...devices.value, ...response.data];
    } else {
      devices.value = response.data;
    }
    
    totalDevices.value = response.total;
    hasMore.value = page < response.totalPages;
    currentPage.value = page;
    
    console.log(`Loaded page ${page}: ${response.data.length} devices, total: ${response.total}`);
  } catch (err) {
    console.error('Error loading devices:', err);
    error.value = 'Failed to load devices. Please try again.';
    devices.value = [];
    hasMore.value = false;
  } finally {
    loading.value = false;
    loadingMore.value = false;
  }
};

// Debounced search function
const performSearch = (query: string) => {
  if (searchTimeout) {
    clearTimeout(searchTimeout);
  }
  
  searchTimeout = setTimeout(() => {
    currentPage.value = 1;
    if (query.trim()) {
      // When searching, load all devices (including requester's) in search results
      devices.value = [];
      requesterDevices.value = [];
      loadDevices(1, query, false);
    } else {
      // When clearing search, reload requester devices separately
      loadRequesterDevices();
      loadDevices(1, '', false);
    }
  }, searchDebounceMs);
};

// Watch for search query changes
watch(searchQuery, (newQuery) => {
  performSearch(newQuery);
});

// Load more devices when scrolling near bottom
const handleScroll = () => {
  if (!scrollContainer.value || loadingMore.value || !hasMore.value) return;
  
  const { scrollTop, scrollHeight, clientHeight } = scrollContainer.value;
  const scrollPercentage = (scrollTop + clientHeight) / scrollHeight;
  
  // Load more when 80% scrolled
  if (scrollPercentage > 0.8) {
    loadMore();
  }
};

// Load next page
const loadMore = async () => {
  if (!hasMore.value || loadingMore.value) return;
  
  await loadDevices(currentPage.value + 1, searchQuery.value, true);
};

// Computed properties for device sorting and display
const allDevicesForDisplay = computed(() => {
  // Combine requester devices (at top) with paginated devices
  const combinedDevices = [
    ...requesterDevices.value.map(device => ({ ...device, isRequesterDevice: true })),
    ...devices.value.map(device => ({ ...device, isRequesterDevice: false }))
  ];
  
  if (import.meta.env.DEV) {
    console.log('DeviceSelectionModal: allDevicesForDisplay computed:', {
      requesterDevicesCount: requesterDevices.value.length,
      paginatedDevicesCount: devices.value.length,
      totalCombined: combinedDevices.length,
      combinedDevices
    });
  }
  
  return combinedDevices;
});

// Get total count including requester devices
const totalDevicesCount = computed(() => {
  return totalDevices.value + requesterDevices.value.length;
});

// Check if any devices available to show
const hasDevicesToShow = computed(() => {
  return requesterDevices.value.length > 0 || devices.value.length > 0;
});

// Watch for modal visibility
watch(() => props.show, (newValue) => {
  if (newValue) {
    if (import.meta.env.DEV) {
      console.log('DeviceSelectionModal: Modal opened, initializing...', {
        requesterUuid: props.requesterUuid,
        existingDeviceIds: props.existingDeviceIds,
        currentTicketId: props.currentTicketId
      });
    }
    
    // Reset state
    devices.value = [];
    requesterDevices.value = [];
    searchQuery.value = '';
    currentPage.value = 1;
    hasMore.value = false;
    totalDevices.value = 0;
    error.value = null;
    
    // Load initial data
    nextTick(() => {
      if (import.meta.env.DEV) {
        console.log('DeviceSelectionModal: Starting data load in nextTick');
      }
      loadRequesterDevices();
      loadDevices(1, '', false);
    });
  } else {
    // Clear search timeout when modal closes
    if (searchTimeout) {
      clearTimeout(searchTimeout);
      searchTimeout = null;
    }
  }
});

const selectDevice = (device: Device) => {
  emit('select-device', device);
  emit('close');
};

// Get device type based on manufacturer and model
const getDeviceType = (device: Device): string => {
  const manufacturer = device.manufacturer?.toLowerCase() || '';
  const model = device.model?.toLowerCase() || '';
  
  if (manufacturer.includes('microsoft') && model.includes('surface')) {
    return 'Surface';
  } else if (model.includes('laptop') || model.includes('thinkpad') || model.includes('latitude')) {
    return 'Laptop';
  } else if (model.includes('desktop') || model.includes('optiplex') || model.includes('thinkcentre')) {
    return 'Desktop';
  } else if (model.includes('tablet') || model.includes('ipad')) {
    return 'Tablet';
  } else if (model.includes('phone') || model.includes('iphone')) {
    return 'Mobile';
  }
  return 'Computer';
};

const getDeviceTypeClass = (device: Device) => {
  const type = getDeviceType(device);
  switch (type) {
    case 'Surface':
      return 'bg-status-info-muted text-status-info border-status-info/30';
    case 'Laptop':
      return 'bg-accent-muted text-accent border-accent/30';
    case 'Desktop':
      return 'bg-status-success-muted text-status-success border-status-success/30';
    case 'Tablet':
      return 'bg-status-warning-muted text-status-warning border-status-warning/30';
    case 'Mobile':
      return 'bg-status-error-muted text-status-error border-status-error/30';
    default:
      return 'bg-surface-alt text-secondary border-default';
  }
};

// Get warranty status styling
const getWarrantyStatusClass = (status: string) => {
  switch (status.toLowerCase()) {
    case 'active':
    case 'compliant':
      return 'bg-status-success-muted text-status-success border-status-success/30';
    case 'warning':
    case 'noncompliant':
      return 'bg-status-warning-muted text-status-warning border-status-warning/30';
    case 'expired':
    case 'error':
      return 'bg-status-error-muted text-status-error border-status-error/30';
    default:
      return 'bg-surface-alt text-secondary border-default';
  }
};

// Format last updated date
const formatLastUpdated = (dateString: string): string => {
  try {
    const date = new Date(dateString);
    const now = new Date();
    const diffTime = now.getTime() - date.getTime();
    const diffMinutes = Math.floor(diffTime / (1000 * 60));

    if (diffMinutes < 1) {
      return 'just now';
    } else if (diffMinutes < 60) {
      return `${diffMinutes}m ago`;
    } else if (diffMinutes < 1440) {
      const hours = Math.floor(diffMinutes / 60);
      return `${hours}h ago`;
    } else {
      const days = Math.floor(diffMinutes / 1440);
      return `${days}d ago`;
    }
  } catch (e) {
    return 'unknown';
  }
};
</script>

<template>
  <Modal :show="show" title="Select a Device" @close="emit('close')" size="lg">
    <div class="flex flex-col gap-4">
      <!-- Search -->
      <div class="relative">
        <div class="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
          <svg class="h-5 w-5 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
          </svg>
        </div>
        <input
          type="text"
          v-model="searchQuery"
          class="w-full pl-10 pr-4 py-2.5 rounded-lg border border-default bg-surface-alt text-primary placeholder-tertiary transition-colors duration-200 hover:border-strong focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
          placeholder="Search devices by name, hostname, serial number, manufacturer, or user..."
        >
        <div v-if="loading && searchQuery" class="absolute inset-y-0 right-0 pr-3 flex items-center">
          <svg class="w-5 h-5 animate-spin text-tertiary" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="m4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
        </div>
      </div>

      <!-- Search hint -->
      <EmptyState
        v-if="!searchQuery && !loading && devices.length === 0"
        icon="search"
        title="Search for devices"
        description="Start typing to find devices by name, serial number, or user"
        variant="compact"
      />

      <!-- Loading state (initial load) -->
      <div v-else-if="loading && devices.length === 0" class="text-center py-8 text-tertiary">
        <div class="inline-flex items-center gap-3">
          <svg class="w-5 h-5 animate-spin" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="m4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
          <span>Loading devices...</span>
        </div>
      </div>

      <!-- Error state -->
      <div v-else-if="error" class="text-center py-8">
        <div class="bg-status-error-muted border border-status-error/30 rounded-lg p-4">
          <p class="text-status-error flex items-center justify-center gap-2">
            <svg class="w-5 h-5 flex-shrink-0" fill="currentColor" viewBox="0 0 20 20">
              <path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7 4a1 1 0 11-2 0 1 1 0 012 0zm-1-9a1 1 0 00-1 1v4a1 1 0 102 0V6a1 1 0 00-1-1z" clip-rule="evenodd" />
            </svg>
            {{ error }}
          </p>
          <button
            @click="loadDevices(1, searchQuery, false)"
            class="mt-3 px-4 py-2 bg-status-error text-white rounded-md hover:opacity-90 transition-colors text-sm"
          >
            Try Again
          </button>
        </div>
      </div>

      <!-- No results -->
      <EmptyState
        v-else-if="!loading && allDevicesForDisplay.length === 0 && searchQuery"
        icon="device"
        title="No devices found"
        description="Try adjusting your search criteria"
        variant="compact"
      />

      <!-- Devices list -->
      <div
        v-else-if="hasDevicesToShow"
        ref="scrollContainer"
        @scroll="handleScroll"
        class="-mx-4 sm:mx-0"
      >
        <!-- Mobile: Card list -->
        <div class="divide-y divide-default sm:hidden">
          <div
            v-for="device in allDevicesForDisplay"
            :key="device.id"
            class="p-4 active:bg-surface-hover transition-colors cursor-pointer"
            :class="{ 'bg-accent/5 border-l-4 border-accent': device.isRequesterDevice }"
            @click="selectDevice(device)"
          >
            <!-- Header: Name + Badge -->
            <div class="flex items-start justify-between gap-2 mb-2">
              <div class="min-w-0 flex-1">
                <div class="font-medium text-primary truncate">{{ device.name }}</div>
                <div class="text-xs text-tertiary truncate">{{ device.hostname }}</div>
              </div>
              <div v-if="device.isRequesterDevice" class="flex-shrink-0">
                <span class="text-xs bg-accent text-white px-2 py-0.5 rounded-full">Owner</span>
              </div>
            </div>

            <!-- Badges row -->
            <div class="flex flex-wrap gap-1.5 mb-3">
              <span
                class="text-xs px-2 py-0.5 rounded-full border"
                :class="getDeviceTypeClass(device)"
              >
                {{ getDeviceType(device) }}
              </span>
              <span
                class="text-xs px-2 py-0.5 rounded-full border capitalize"
                :class="getWarrantyStatusClass(device.warranty_status)"
              >
                {{ device.warranty_status }}
              </span>
              <span v-if="device.intune_device_id" class="text-xs px-2 py-0.5 rounded-full bg-status-info/20 text-status-info border border-status-info/30">
                Intune
              </span>
            </div>

            <!-- Details -->
            <div class="grid grid-cols-2 gap-2 text-xs">
              <div>
                <span class="text-tertiary">Serial:</span>
                <span class="text-secondary font-mono ml-1">{{ device.serial_number || '—' }}</span>
              </div>
              <div>
                <span class="text-tertiary">Model:</span>
                <span class="text-secondary ml-1">{{ device.model || '—' }}</span>
              </div>
            </div>

            <!-- User + Updated -->
            <div class="flex items-center justify-between mt-3 pt-3 border-t border-subtle">
              <div v-if="device.primary_user" class="flex items-center gap-2">
                <UserAvatar
                  :name="device.primary_user.uuid"
                  :userName="device.primary_user.name"
                  :avatar="device.primary_user.avatar_thumb || device.primary_user.avatar_url"
                  size="xs"
                  :showName="true"
                  :clickable="false"
                />
              </div>
              <span v-else class="text-xs text-tertiary">Unassigned</span>
              <span class="text-xs text-tertiary">{{ formatLastUpdated(device.updated_at) }}</span>
            </div>
          </div>
        </div>

        <!-- Desktop: Table layout -->
        <div class="hidden sm:block rounded-lg border border-default overflow-hidden">
          <table class="w-full">
            <thead class="bg-surface-alt text-xs font-medium text-secondary uppercase tracking-wide">
              <tr>
                <th class="px-4 py-3 text-left">Device</th>
                <th class="px-4 py-3 text-left w-44">Status</th>
                <th class="px-4 py-3 text-left w-32">Serial</th>
                <th class="px-4 py-3 text-left w-40">User</th>
                <th class="px-4 py-3 text-right w-20"></th>
              </tr>
            </thead>
            <tbody class="divide-y divide-subtle">
              <tr
                v-for="device in allDevicesForDisplay"
                :key="device.id"
                class="group hover:bg-surface-hover transition-colors cursor-pointer"
                :class="{ 'bg-accent/5': device.isRequesterDevice }"
                @click="selectDevice(device)"
              >
                <td class="px-4 py-3">
                  <div class="flex items-center gap-3">
                    <div class="min-w-0">
                      <div class="flex items-center gap-2">
                        <span class="font-medium text-primary truncate">{{ device.name }}</span>
                        <span v-if="device.isRequesterDevice" class="text-xs bg-accent text-white px-1.5 py-0.5 rounded">Owner</span>
                      </div>
                      <div class="text-xs text-tertiary truncate">{{ device.manufacturer }} {{ device.model }}</div>
                    </div>
                  </div>
                </td>
                <td class="px-4 py-3">
                  <div class="flex flex-wrap gap-1">
                    <span class="text-xs px-2 py-0.5 rounded-full border" :class="getDeviceTypeClass(device)">
                      {{ getDeviceType(device) }}
                    </span>
                    <span class="text-xs px-2 py-0.5 rounded-full border capitalize" :class="getWarrantyStatusClass(device.warranty_status)">
                      {{ device.warranty_status }}
                    </span>
                  </div>
                </td>
                <td class="px-4 py-3">
                  <span class="text-xs text-secondary font-mono">{{ device.serial_number || '—' }}</span>
                </td>
                <td class="px-4 py-3">
                  <UserAvatar
                    v-if="device.primary_user"
                    :name="device.primary_user.uuid"
                    :userName="device.primary_user.name"
                    :avatar="device.primary_user.avatar_thumb || device.primary_user.avatar_url"
                    size="sm"
                    :showName="true"
                    :clickable="false"
                  />
                  <span v-else class="text-xs text-tertiary">—</span>
                </td>
                <td class="px-4 py-3 text-right">
                  <button class="text-accent text-xs font-medium px-2 py-1 rounded hover:bg-accent/10 transition-colors">
                    Select
                  </button>
                </td>
              </tr>
            </tbody>
          </table>
        </div>

        <!-- Load more indicator -->
        <div v-if="loadingMore" class="p-4 text-center">
          <div class="inline-flex items-center gap-2 text-tertiary text-sm">
            <svg class="w-4 h-4 animate-spin" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="m4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
            <span>Loading...</span>
          </div>
        </div>
      </div>
    </div>

    <!-- Footer -->
    <div class="flex flex-col-reverse sm:flex-row items-stretch sm:items-center justify-between gap-3 pt-4 mt-4">
      <button
        type="button"
        class="flex items-center justify-center gap-2 px-4 py-2 text-sm text-accent hover:bg-accent/10 rounded-lg transition-colors"
        @click="$router.push('/devices/new')"
      >
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6v6m0 0v6m0-6h6m-6 0H6" />
        </svg>
        New Device
      </button>

      <div class="flex items-center justify-between sm:justify-end gap-3">
        <span class="text-xs text-tertiary">
          {{ totalDevicesCount }} device{{ totalDevicesCount !== 1 ? 's' : '' }}
        </span>
        <button
          type="button"
          class="px-4 py-2 text-sm text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
          @click="emit('close')"
        >
          Cancel
        </button>
      </div>
    </div>
  </Modal>
</template> 