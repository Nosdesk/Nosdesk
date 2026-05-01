<script setup lang="ts">
import { formatDateTime } from '@/utils/dateUtils';
import { ref, computed, onMounted, onUnmounted } from "vue";
import apiClient from "@/services/apiConfig";
import BackButton from "@/components/common/BackButton.vue";
import Modal from "@/components/Modal.vue";
import AlertMessage from "@/components/common/AlertMessage.vue";
import Checkbox from "@/components/common/Checkbox.vue";
import Icon from "@/components/common/Icon.vue";
import Spinner from "@/components/common/Spinner.vue";
import { AdminIcons } from "@/components/admin/AdminIcons";
import type {
  ConfigValidation,
  SyncResult,
  ActiveSync,
  LastSyncDetails,
} from "@/types";

// Connection state
const connectionStatus = ref<
  "connected" | "disconnected" | "connecting" | "error"
>("disconnected");
const lastSync = ref<string | null>(null);
const errorMessage = ref<string | null>(null);
const successMessage = ref<string | null>(null);
const isLoading = ref(false);
const syncResults = ref<SyncResult | null>(null);

// Progress tracking state (ActiveSync from backend)
const syncProgress = ref<ActiveSync | null>(null);
const currentSessionId = ref<string | null>(null);
const progressPollingInterval = ref<ReturnType<typeof setInterval> | null>(null);
const isSyncing = ref(false);

// Active syncs management
const activeSyncs = ref<ActiveSync[]>([]);
const isLoadingActiveSyncs = ref(false);

// Last sync details
const lastSyncDetails = ref<LastSyncDetails | null>(null);
const isLoadingLastSync = ref(false);

// Configuration validation state
const configValidation = ref<ConfigValidation | null>(null);
const isValidatingConfig = ref(false);

// Import options
const selectedEntities = ref(["users", "devices", "groups"]);
const fullSyncMode = ref(false);
const availableEntities = [
  {
    id: "users",
    name: "Users",
    description: "Import user accounts and profiles from Microsoft Entra ID",
  },
  {
    id: "devices",
    name: "Devices",
    description: "Import managed devices from Microsoft Intune with user assignments",
  },
  {
    id: "groups",
    name: "Groups",
    description: "Import security and distribution groups from Microsoft Entra ID",
  },
];

// Modals
const showSyncModal = ref(false);

// Configuration validation
const validateConfiguration = async () => {
  isValidatingConfig.value = true;
  errorMessage.value = null;

  try {
    const response = await apiClient.get("/integrations/graph/config");
    configValidation.value = response.data;
  } catch (error) {
    console.error("Failed to validate configuration:", error);
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || "Failed to validate configuration";
    configValidation.value = null;
  } finally {
    isValidatingConfig.value = false;
  }
};

// Fetch current connection status
const fetchConnectionStatus = async () => {
  isLoading.value = true;

  try {
    const response = await apiClient.get("/integrations/graph/status");
    connectionStatus.value = response.data.status;

    if (response.data.last_sync) {
      lastSync.value = formatDateTime(response.data.last_sync);
    }

    // Validate configuration on load
    await validateConfiguration();
  } catch (error) {
    console.error("Failed to fetch MS Graph connection status:", error);
    errorMessage.value = "Failed to fetch connection status";
    connectionStatus.value = "error";

    // Still try to validate configuration even if status fetch fails
    await validateConfiguration();
  } finally {
    isLoading.value = false;
  }
};

// Sync data
const syncData = () => {
  showSyncModal.value = true;
};

// Toggle entity selection
const toggleEntity = (entityId: string) => {
  if (selectedEntities.value.includes(entityId)) {
    selectedEntities.value = selectedEntities.value.filter(
      (id) => id !== entityId
    );
  } else {
    selectedEntities.value.push(entityId);
  }
};

// Progress polling functions
const startProgressPolling = (sessionId: string) => {
  currentSessionId.value = sessionId;
  isSyncing.value = true;
  syncProgress.value = null;

  progressPollingInterval.value = setInterval(async () => {
    try {
      const response = await apiClient.get(`/integrations/graph/progress/${sessionId}`);
      syncProgress.value = response.data;

      if (response.data.status === 'completed' ||
          response.data.status === 'error' ||
          response.data.status === 'cancelled' ||
          response.data.status === 'completed_with_errors') {
        stopProgressPolling(true);
      }
    } catch (error) {
      console.error("Failed to fetch sync progress:", error);
      const axiosError = error as { response?: { status?: number } };
      if (axiosError?.response?.status === 404) {
        stopProgressPolling(true);
      }
    }
  }, 1000);
};

const stopProgressPolling = (refreshData = false) => {
  if (progressPollingInterval.value) {
    clearInterval(progressPollingInterval.value);
    progressPollingInterval.value = null;
  }
  isSyncing.value = false;
  currentSessionId.value = null;

  if (refreshData) {
    fetchActiveSyncs();
    fetchLastSyncDetails();
  }
};

// Cancel a sync session
const cancelSync = async (sessionId: string) => {
  try {
    const response = await apiClient.post(`/integrations/graph/cancel/${sessionId}`);

    if (response.data.success) {
      successMessage.value = response.data.message || "Sync cancellation requested";

      if (currentSessionId.value === sessionId) {
        stopProgressPolling();
      }
    } else {
      errorMessage.value = response.data.message || "Failed to cancel sync";
    }

    setTimeout(() => {
      successMessage.value = null;
      errorMessage.value = null;
    }, 3000);
  } catch (error) {
    console.error("Failed to cancel sync:", error);
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || "Failed to cancel sync";

    setTimeout(() => {
      errorMessage.value = null;
    }, 3000);
  }
};

// Start sync process with specified mode
const startSyncWithMode = async (useDelta: boolean) => {
  isLoading.value = true;
  errorMessage.value = null;
  syncResults.value = null;

  try {
    const response = await apiClient.post("/integrations/graph/sync", {
      entities: selectedEntities.value,
      use_delta: useDelta,
    });

    showSyncModal.value = false;

    if (response.data.success && response.data.session_id) {
      successMessage.value = response.data.message || "Sync started successfully";
      startProgressPolling(response.data.session_id);
    } else {
      errorMessage.value = response.data.message || "Failed to start sync";
    }

    setTimeout(() => {
      successMessage.value = null;
    }, 3000);
  } catch (error) {
    console.error("Failed to start sync:", error);
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value =
      axiosError.response?.data?.message || "Failed to start sync";
  } finally {
    isLoading.value = false;
  }
};

// Cleanup on component unmount
onUnmounted(() => {
  stopProgressPolling();
});

// Format sync type for display
const formatSyncType = (syncType?: string): string => {
  const type = syncType ?? 'unknown';
  switch (type) {
    case 'users': return 'User Accounts';
    case 'profile_photos': return 'Profile Photos';
    case 'devices': return 'Managed Devices';
    case 'groups': return 'Security Groups';
    default: return type.charAt(0).toUpperCase() + type.slice(1);
  }
};

// Format time ago
const formatTimeAgo = (dateString: string) => {
  const date = new Date(dateString);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMins = Math.floor(diffMs / 60000);

  if (diffMins < 1) return 'Just now';
  if (diffMins < 60) return `${diffMins}m ago`;

  const diffHours = Math.floor(diffMins / 60);
  if (diffHours < 24) return `${diffHours}h ago`;

  const diffDays = Math.floor(diffHours / 24);
  return `${diffDays}d ago`;
};

// Format duration between two dates
const formatDuration = (startDate: string, endDate: string) => {
  const start = new Date(startDate);
  const end = new Date(endDate);
  const diffMs = end.getTime() - start.getTime();
  const diffSeconds = Math.floor(diffMs / 1000);

  if (diffSeconds < 60) return `${diffSeconds}s`;

  const diffMins = Math.floor(diffSeconds / 60);
  if (diffMins < 60) return `${diffMins}m ${diffSeconds % 60}s`;

  const diffHours = Math.floor(diffMins / 60);
  return `${diffHours}h ${diffMins % 60}m`;
};

// Overall progress computed from completed_items + current entity progress
const overallProgress = computed(() => {
  if (!syncProgress.value) return 0;
  const s = syncProgress.value;
  const totalOverall = (s.completed_items || 0) + s.total;
  if (totalOverall === 0) return 0;
  return Math.round(((s.completed_items || 0) + s.current) / totalOverall * 100);
});

const overallProgressCounts = computed(() => {
  if (!syncProgress.value) return { current: 0, total: 0 };
  const s = syncProgress.value;
  return {
    current: (s.completed_items || 0) + s.current,
    total: (s.completed_items || 0) + s.total,
  };
});

// Entity step tracking for multi-entity syncs
const currentEntityIndex = computed(() => {
  if (!syncProgress.value) return 0;
  const entity = syncProgress.value.entity;
  const entityOrder = ['users', 'devices', 'groups'];
  const idx = entityOrder.indexOf(entity);
  return idx >= 0 ? idx + 1 : 1;
});

const totalEntities = computed(() => selectedEntities.value.length);

// Fetch active syncs
const fetchActiveSyncs = async () => {
  isLoadingActiveSyncs.value = true;
  try {
    const response = await apiClient.get("/integrations/graph/active-syncs");
    activeSyncs.value = response.data.active_syncs || [];

    if (activeSyncs.value.length > 0 && !isSyncing.value) {
      const runningSyncs = activeSyncs.value.filter(sync =>
        sync.status === 'running' || sync.status === 'starting'
      );

      if (runningSyncs.length > 0) {
        startProgressPolling(runningSyncs[0].session_id);
      }
    }
  } catch (error) {
    console.error("Failed to fetch active syncs:", error);
  } finally {
    isLoadingActiveSyncs.value = false;
  }
};

// Resume monitoring an active sync
const resumeSync = (sessionId: string) => {
  if (currentSessionId.value !== sessionId) {
    stopProgressPolling();
    startProgressPolling(sessionId);
  }
};

// Fetch last sync details
const fetchLastSyncDetails = async () => {
  isLoadingLastSync.value = true;
  try {
    const response = await apiClient.get("/integrations/graph/last-sync");
    lastSyncDetails.value = response.data ?? null;
  } catch (error) {
    console.error("Failed to fetch last sync details:", error);
    lastSyncDetails.value = null;
  } finally {
    isLoadingLastSync.value = false;
  }
};

onMounted(async () => {
  await fetchConnectionStatus();
  await fetchActiveSyncs();
  await fetchLastSyncDetails();
});
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <!-- Header -->
      <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3 mb-2">
        <div>
          <BackButton
            fallbackRoute="/admin/data-import"
            label="Back to Data Import"
          />
          <h1 class="text-xl sm:text-2xl font-bold text-primary mt-2">Microsoft Graph</h1>
          <p class="text-secondary text-sm sm:text-base mt-1">Manage data synchronization from Microsoft 365 services</p>
        </div>

        <!-- Sync button -->
        <button
          @click="syncData"
          :disabled="connectionStatus !== 'connected' || isLoading || isSyncing || !!(configValidation && !configValidation.valid)"
          class="self-start sm:self-auto px-4 py-2 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2 whitespace-nowrap"
        >
          <Spinner v-if="isSyncing" />
          <Icon v-else name="refresh" />
          {{ isSyncing ? 'Syncing...' : 'Sync Data' }}
        </button>
      </div>

      <!-- Alerts -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Content -->
      <div class="flex flex-col gap-4">
        <!-- Configuration & Connection card -->
        <div class="bg-surface border border-default rounded-xl hover:border-strong transition-colors overflow-hidden">
          <div class="p-4 flex flex-col gap-3">
            <!-- Header row with icon -->
            <div class="flex items-center gap-3">
              <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-accent/20 flex items-center justify-center text-accent">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" v-html="AdminIcons.microsoft"></svg>
              </div>

              <div class="flex-1 flex items-center gap-2 flex-wrap">
                <span class="font-medium text-primary">Microsoft Graph API</span>
                <!-- Skeleton badges while loading -->
                <template v-if="isLoading && !configValidation">
                  <span class="h-5 w-20 bg-surface-hover rounded-full animate-pulse"></span>
                  <span class="h-5 w-24 bg-surface-hover rounded-full animate-pulse"></span>
                </template>
                <!-- Real badges -->
                <template v-else>
                  <span
                    class="px-1.5 py-0.5 text-xs rounded-full border"
                    :class="{
                      'bg-status-success/20 text-status-success border-status-success/50': connectionStatus === 'connected',
                      'bg-surface-alt text-tertiary border-default': connectionStatus === 'disconnected',
                      'bg-accent/20 text-accent border-accent/50': connectionStatus === 'connecting',
                      'bg-status-error/20 text-status-error border-status-error/50': connectionStatus === 'error'
                    }"
                  >
                    {{ connectionStatus === 'connected' ? 'Connected' : connectionStatus === 'disconnected' ? 'Not Connected' : connectionStatus === 'connecting' ? 'Connecting...' : 'Error' }}
                  </span>
                  <span
                    v-if="configValidation"
                    class="px-1.5 py-0.5 text-xs rounded-full border"
                    :class="configValidation.valid
                      ? 'bg-status-success/20 text-status-success border-status-success/50'
                      : 'bg-status-error/20 text-status-error border-status-error/50'"
                  >
                    {{ configValidation.valid ? 'Configured' : 'Not Configured' }}
                  </span>
                </template>
              </div>
            </div>

            <!-- Skeleton config values while loading -->
            <div v-if="isLoading && !configValidation" class="flex flex-col md:flex-row gap-4 text-sm animate-pulse">
              <div class="flex-1 flex flex-col gap-2">
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">Client ID</span>
                  <div class="h-7 bg-surface-hover rounded w-full"></div>
                </div>
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">Tenant ID</span>
                  <div class="h-7 bg-surface-hover rounded w-full"></div>
                </div>
              </div>
              <div class="flex flex-row md:flex-col gap-4 md:gap-2 md:w-28 md:flex-shrink-0">
                <div class="flex flex-col gap-0.5">
                  <span class="text-tertiary text-xs">Secret</span>
                  <div class="h-7 bg-surface-hover rounded w-full"></div>
                </div>
              </div>
            </div>

            <!-- Real config values -->
            <template v-else>
              <div v-if="configValidation" class="flex flex-col md:flex-row gap-4 text-sm">
                <div class="flex-1 flex flex-col gap-2">
                  <div class="flex flex-col gap-0.5">
                    <span class="text-tertiary text-xs">Client ID</span>
                    <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ configValidation.client_id || 'Not set' }}</span>
                  </div>
                  <div class="flex flex-col gap-0.5">
                    <span class="text-tertiary text-xs">Tenant ID</span>
                    <span class="text-primary font-mono text-xs bg-surface-alt px-2 py-1.5 rounded select-all break-all">{{ configValidation.tenant_id || 'Not set' }}</span>
                  </div>
                </div>
                <div class="flex flex-row md:flex-col gap-4 md:gap-2 md:w-28 md:flex-shrink-0">
                  <div class="flex flex-col gap-0.5">
                    <span class="text-tertiary text-xs">Secret</span>
                    <span :class="configValidation.client_secret_configured ? 'text-status-success' : 'text-status-error'" class="font-medium bg-surface-alt px-2 py-1.5 rounded text-xs">{{ configValidation.client_secret_configured ? 'Configured' : 'Not Set' }}</span>
                  </div>
                </div>
              </div>

              <!-- Last sync info -->
              <div v-if="lastSync" class="flex items-center gap-2 text-xs">
                <span class="text-tertiary">Last synchronized:</span>
                <span class="text-primary">{{ lastSync }}</span>
              </div>

              <!-- Configuration Issues -->
              <div
                v-if="configValidation?.missing_fields && configValidation.missing_fields.length > 0"
                class="p-2 bg-status-error/10 border border-status-error/30 rounded-lg text-sm text-status-error flex items-start gap-2"
              >
                <Icon name="info" class="flex-shrink-0 mt-0.5" />
                <div>
                  Missing required configuration:
                  <span v-for="(field, index) in configValidation.missing_fields" :key="field">
                    {{ field }}<span v-if="index < configValidation.missing_fields.length - 1">, </span>
                  </span>
                </div>
              </div>
            </template>

            <!-- Required environment variables -->
            <div class="flex items-center gap-2 text-xs">
              <span class="text-tertiary">Env:</span>
              <div class="flex flex-wrap gap-1">
                <code class="bg-surface-alt text-secondary px-1 py-0.5 rounded">MICROSOFT_CLIENT_ID</code>
                <code class="bg-surface-alt text-secondary px-1 py-0.5 rounded">MICROSOFT_CLIENT_SECRET</code>
                <code class="bg-surface-alt text-secondary px-1 py-0.5 rounded">MICROSOFT_TENANT_ID</code>
                <code class="bg-surface-alt text-secondary px-1 py-0.5 rounded">MICROSOFT_REDIRECT_URI</code>
              </div>
            </div>
          </div>

        </div>

        <!-- Sync Progress (real-time monitoring) -->
        <div
          v-if="isSyncing && syncProgress"
          class="bg-surface border border-default rounded-xl overflow-hidden"
        >
          <div class="p-4 flex flex-col gap-3">
            <!-- Header with step indicator and cancel -->
            <div class="flex items-center gap-3">
              <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-accent/20 flex items-center justify-center text-accent">
                <Spinner size="md" />
              </div>

              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2 flex-wrap">
                  <span class="font-medium text-primary">Synchronizing</span>
                  <span v-if="syncProgress.entity !== 'initializing'" class="text-xs text-tertiary">Step {{ currentEntityIndex }} of {{ totalEntities }}</span>
                  <span
                    class="px-1.5 py-0.5 text-xs rounded-full border"
                    :class="{
                      'bg-accent/20 text-accent border-accent/50': syncProgress.status === 'running' || syncProgress.status === 'starting',
                      'bg-status-success/20 text-status-success border-status-success/50': syncProgress.status === 'completed',
                      'bg-status-warning/20 text-status-warning border-status-warning/50': syncProgress.status === 'completed_with_errors' || syncProgress.status === 'cancelling',
                      'bg-status-error/20 text-status-error border-status-error/50': syncProgress.status === 'error',
                      'bg-surface-alt text-tertiary border-default': syncProgress.status === 'cancelled'
                    }"
                  >
                    {{ syncProgress.status === 'completed_with_errors' ? 'Completed with errors' : syncProgress.status }}
                  </span>
                </div>
                <div class="text-xs text-secondary mt-0.5 truncate">{{ syncProgress.message }}</div>
              </div>

              <button
                v-if="(syncProgress.status === 'running' || syncProgress.status === 'starting') && currentSessionId"
                @click="cancelSync(currentSessionId!)"
                class="px-3 py-1.5 bg-status-error/20 text-status-error border border-status-error/50 rounded-lg text-sm hover:bg-status-error/30 font-medium transition-colors flex items-center gap-1.5 whitespace-nowrap flex-shrink-0"
              >
                Cancel
              </button>
            </div>

            <!-- Overall progress bar -->
            <div>
              <div class="flex items-center justify-between text-xs mb-1.5">
                <span class="text-secondary font-medium">{{ formatSyncType(syncProgress.entity) }}</span>
                <span class="text-primary font-medium tabular-nums">{{ overallProgressCounts.current }} / {{ overallProgressCounts.total }} <span class="text-tertiary">({{ overallProgress }}%)</span></span>
              </div>
              <div class="w-full bg-surface-alt rounded-full h-2">
                <div
                  class="h-2 rounded-full transition-all duration-300"
                  :class="{
                    'bg-accent': syncProgress.status === 'running' || syncProgress.status === 'starting',
                    'bg-status-success': syncProgress.status === 'completed',
                    'bg-status-warning': syncProgress.status === 'completed_with_errors' || syncProgress.status === 'cancelling',
                    'bg-status-error': syncProgress.status === 'error',
                    'bg-surface-hover': syncProgress.status === 'cancelled'
                  }"
                  :style="{
                    width: overallProgressCounts.total > 0
                      ? `${overallProgress}%`
                      : (syncProgress.status === 'completed' || syncProgress.status === 'completed_with_errors') ? '100%' : '0%'
                  }"
                ></div>
              </div>
            </div>

            <!-- Entity-level detail (current entity progress) -->
            <div v-if="syncProgress.entity !== 'initializing' && syncProgress.total > 0" class="flex items-center gap-3 text-xs text-tertiary">
              <span>{{ formatSyncType(syncProgress.entity) }}: {{ syncProgress.current }} / {{ syncProgress.total }}</span>
              <span v-if="syncProgress.is_delta" class="px-1 py-0.5 bg-surface-alt rounded text-tertiary">Delta</span>
            </div>
          </div>
        </div>

        <!-- Active sync notice (when not monitoring but syncs exist) -->
        <div
          v-else-if="activeSyncs.length > 0 && !isSyncing"
          class="bg-surface border border-accent/30 rounded-xl overflow-hidden"
        >
          <div class="p-4 flex flex-col gap-2">
            <div
              v-for="sync in activeSyncs"
              :key="sync.session_id"
              class="flex items-center gap-3 cursor-pointer hover:bg-surface-alt/50 -mx-1 px-1 py-1 rounded-lg transition-colors"
              @click="resumeSync(sync.session_id)"
            >
              <div class="flex-shrink-0 h-7 w-7 rounded-lg bg-accent/20 flex items-center justify-center text-accent">
                <Spinner />
              </div>
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2">
                  <span class="text-sm font-medium text-primary">{{ formatSyncType(sync.sync_type) }}</span>
                  <span class="text-xs text-tertiary">{{ formatTimeAgo(sync.started_at) }}</span>
                </div>
                <div class="text-xs text-secondary truncate">{{ sync.message }}</div>
              </div>
              <div class="flex items-center gap-2 flex-shrink-0">
                <span class="text-xs text-accent font-medium">Monitor</span>
                <button
                  @click.stop="cancelSync(sync.session_id)"
                  class="px-2 py-1 bg-status-error/10 text-status-error rounded text-xs hover:bg-status-error/20 transition-colors"
                >
                  Cancel
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- Last Sync Details -->
        <div
          v-if="lastSyncDetails || isLoadingLastSync"
          class="bg-surface border border-default rounded-xl overflow-hidden"
        >
          <div class="p-4 flex flex-col gap-3">
            <div class="flex items-center gap-3">
              <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-accent/20 flex items-center justify-center text-accent">
                <Icon name="clock" size="md" />
              </div>

              <div class="flex-1 flex items-center gap-2 flex-wrap">
                <span class="font-medium text-primary">Last Synchronization</span>
                <span
                  v-if="lastSyncDetails"
                  class="px-1.5 py-0.5 text-xs rounded-full border"
                  :class="{
                    'bg-status-success/20 text-status-success border-status-success/50': lastSyncDetails.status === 'completed',
                    'bg-status-warning/20 text-status-warning border-status-warning/50': lastSyncDetails.status === 'completed_with_errors',
                    'bg-status-error/20 text-status-error border-status-error/50': lastSyncDetails.status === 'error',
                    'bg-surface-alt text-tertiary border-default': lastSyncDetails.status === 'cancelled'
                  }"
                >
                  {{ lastSyncDetails.status === 'completed_with_errors' ? 'Completed with Errors' : lastSyncDetails.status.charAt(0).toUpperCase() + lastSyncDetails.status.slice(1) }}
                </span>
              </div>
            </div>

            <!-- Loading skeleton -->
            <div v-if="isLoadingLastSync" class="animate-pulse flex flex-col gap-2">
              <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
                <div v-for="i in 4" :key="i" class="flex flex-col gap-1">
                  <div class="h-3 w-12 bg-surface-hover rounded"></div>
                  <div class="h-5 w-16 bg-surface-hover rounded"></div>
                </div>
              </div>
            </div>

            <!-- Sync Details Content -->
            <div v-else-if="lastSyncDetails">
              <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
                <div class="flex flex-col">
                  <span class="text-tertiary text-xs">Type</span>
                  <span class="text-primary text-sm font-medium">{{ lastSyncDetails.is_delta ? 'Delta' : 'Full' }}</span>
                </div>
                <div class="flex flex-col">
                  <span class="text-tertiary text-xs">Started</span>
                  <span class="text-primary text-sm font-medium">{{ formatTimeAgo(lastSyncDetails.started_at) }}</span>
                </div>
                <div class="flex flex-col">
                  <span class="text-tertiary text-xs">Duration</span>
                  <span class="text-primary text-sm font-medium">{{ formatDuration(lastSyncDetails.started_at, lastSyncDetails.updated_at) }}</span>
                </div>
                <div class="flex flex-col">
                  <span class="text-tertiary text-xs">Items processed</span>
                  <span class="text-primary text-sm font-medium">
                    <span v-if="lastSyncDetails.total > 0">{{ lastSyncDetails.current + (lastSyncDetails.completed_items || 0) }}</span>
                    <span v-else-if="lastSyncDetails.status === 'cancelled'" class="text-tertiary">Cancelled</span>
                    <span v-else-if="lastSyncDetails.status === 'error'" class="text-status-error">Failed</span>
                    <span v-else class="text-tertiary">-</span>
                  </span>
                </div>
              </div>

              <div v-if="lastSyncDetails.message" class="text-xs text-secondary mt-2 bg-surface-alt rounded-lg px-2.5 py-1.5">
                {{ lastSyncDetails.message }}
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Sync Data Modal -->
    <Modal
      :show="showSyncModal"
      title="Sync Data from Microsoft Graph"
      contentClass="max-w-lg"
      @close="showSyncModal = false"
    >
      <div class="flex flex-col gap-4">
        <p class="text-secondary text-sm">
          Select the data entities you want to import from Microsoft Graph:
        </p>

        <div class="flex flex-col gap-2">
          <div
            v-for="entity in availableEntities"
            :key="entity.id"
            class="flex items-center gap-3 p-3 rounded-lg border border-default bg-surface hover:border-strong cursor-pointer transition-colors"
            :class="{ 'border-accent bg-accent/5': selectedEntities.includes(entity.id) }"
            @click="toggleEntity(entity.id)"
          >
            <Checkbox
              :id="`sync-${entity.id}`"
              :model-value="selectedEntities.includes(entity.id)"
              :label="entity.name"
              class="pointer-events-none"
            />
          </div>
        </div>

        <!-- Info notice -->
        <div class="p-3 bg-accent/10 border border-accent/30 rounded-lg text-sm text-accent flex items-start gap-2">
          <Icon name="info" class="flex-shrink-0 mt-0.5" />
          <span>Synchronization will import the latest data from Microsoft services. This may take several minutes depending on the amount of data.</span>
        </div>

        <!-- Sync Progress Results -->
        <div v-if="syncResults" class="p-4 bg-surface-alt rounded-lg border border-default">
          <h4 class="text-sm font-medium text-primary mb-3">Sync Results</h4>

          <div class="flex flex-col gap-2">
            <div
              v-for="result in syncResults.results"
              :key="result.entity"
              class="p-3 rounded-lg border"
              :class="{
                'bg-status-success/10 border-status-success/30': result.status === 'completed',
                'bg-status-warning/10 border-status-warning/30': result.status === 'completed_with_errors',
                'bg-status-error/10 border-status-error/30': result.status === 'error'
              }"
            >
              <div class="flex justify-between items-start">
                <div>
                  <h5 class="font-medium text-primary text-sm capitalize">{{ result.entity }}</h5>
                  <p class="text-xs text-secondary">
                    {{ result.processed }} / {{ result.total }} items
                    <span v-if="result.total > 0" class="text-tertiary">
                      ({{ Math.round((result.processed / result.total) * 100) }}%)
                    </span>
                  </p>
                </div>
                <div class="flex items-center">
                  <Icon v-if="result.status === 'completed'" name="check" class="text-status-success" />
                  <Icon v-else-if="result.status === 'completed_with_errors'" name="warning" class="text-status-warning" />
                  <Icon v-else name="close" class="text-status-error" />
                </div>
              </div>

              <div v-if="result.errors && result.errors.length > 0" class="mt-2 pt-2 border-t border-status-error/20">
                <ul class="text-xs text-status-error flex flex-col gap-1">
                  <li v-for="error in result.errors.slice(0, 3)" :key="error" class="pl-2 border-l-2 border-status-error/50">
                    {{ error }}
                  </li>
                  <li v-if="result.errors.length > 3" class="text-status-error/70 italic pl-2">
                    ... and {{ result.errors.length - 3 }} more errors
                  </li>
                </ul>
              </div>
            </div>
          </div>

          <div class="mt-3 pt-3 border-t border-default">
            <div class="flex justify-between items-center text-sm">
              <span class="text-tertiary">Total processed:</span>
              <span class="font-medium text-primary">{{ syncResults.total_processed }} items</span>
            </div>
            <div v-if="syncResults.total_errors > 0" class="flex justify-between items-center text-sm mt-1">
              <span class="text-tertiary">Total errors:</span>
              <span class="font-medium text-status-error">{{ syncResults.total_errors }}</span>
            </div>
          </div>
        </div>

        <!-- Action buttons -->
        <div class="flex flex-col gap-3 pt-2">
          <Checkbox
            id="full-sync-mode"
            v-model="fullSyncMode"
            label="Full sync"
          />

          <button
            @click="startSyncWithMode(!fullSyncMode)"
            :disabled="isLoading || isSyncing || selectedEntities.length === 0"
            class="w-full px-4 py-2.5 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
          >
            <Spinner v-if="isLoading || isSyncing" />
            <Icon v-else name="refresh" />
            {{ isLoading ? 'Starting...' : isSyncing ? 'Syncing...' : 'Start Sync' }}
          </button>
        </div>
      </div>
    </Modal>
  </div>
</template>
