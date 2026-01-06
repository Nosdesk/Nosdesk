<script setup lang="ts">
import { formatDate, formatDateTime } from '@/utils/dateUtils';
import { ref, onMounted, onUnmounted } from "vue";
import { useRouter } from "vue-router";
import apiClient from "@/services/apiConfig";
import BackButton from "@/components/common/BackButton.vue";
import Modal from "@/components/Modal.vue";
import AlertMessage from "@/components/common/AlertMessage.vue";
import Checkbox from "@/components/common/Checkbox.vue";
import EnvConfigNotice from "@/components/admin/EnvConfigNotice.vue";
import { AdminIcons } from "@/components/admin/AdminIcons";
import type {
  ConfigValidation,
  ConnectionStatus,
  SyncProgress,
  SyncResult,
  ActiveSync,
  LastSyncDetails,
  GraphApiTestResult,
  PermissionTestResult,
} from "@/types";

// Connection state
const connectionStatus = ref<
  "connected" | "disconnected" | "connecting" | "error"
>("disconnected");
const lastSync = ref<string | null>(null);
const errorMessage = ref<string | null>(null);
const successMessage = ref<string | null>(null);
const isLoading = ref(false);
const configurationStatus = ref<ConnectionStatus | null>(null);
const graphApiResults = ref<GraphApiTestResult | null>(null);
const permissionTestResults = ref<PermissionTestResult | null>(null);
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



// Connection details
const graphConfig = ref<{
  clientId: string;
  clientSecret: string;
  tenantId: string;
  scopes: string[];
}>({
  clientId: "",
  clientSecret: "",
  tenantId: "",
  scopes: ["User.Read", "Directory.Read.All", "Device.Read.All"],
});

// Auth provider state
const microsoftAuthProviderFound = ref(false);
const microsoftAuthProviderId = ref<string | null>(null);

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

const router = useRouter();

// Add configuration validation function
const validateConfiguration = async () => {
  isValidatingConfig.value = true;
  errorMessage.value = null;

  try {
    const response = await apiClient.get("/integrations/graph/config");
    configValidation.value = response.data;

    if (!response.data.valid) {
      errorMessage.value = response.data.message || "Configuration validation failed. Check your environment variables.";
    }
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
    // Use the real Microsoft Graph integration endpoint
    const response = await apiClient.get("/integrations/graph/status");
    connectionStatus.value = response.data.status;

    if (response.data.last_sync) {
      lastSync.value = formatDateTime(response.data.last_sync);
    }

    // Load connection details
    if (connectionStatus.value === "connected") {
      graphConfig.value = {
        ...graphConfig.value,
        clientId: response.data.client_id,
        tenantId: response.data.tenant_id,
        // Client secret is not displayed for security reasons
        clientSecret: "",
      };
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
  
  // Poll every 1 second
  progressPollingInterval.value = setInterval(async () => {
    try {
      const response = await apiClient.get(`/integrations/graph/progress/${sessionId}`);
      syncProgress.value = response.data;
      
      // Stop polling if sync is completed, failed, or cancelled
      if (response.data.status === 'completed' ||
          response.data.status === 'error' ||
          response.data.status === 'cancelled' ||
          response.data.status === 'completed_with_errors') {
        stopProgressPolling(true); // Refresh data when sync completes
      }
    } catch (error) {
      console.error("Failed to fetch sync progress:", error);
      // A 404 indicates the session might be completed or expired
      const axiosError = error as { response?: { status?: number } };
      if (axiosError?.response?.status === 404) {
        stopProgressPolling(true); // Refresh data if session not found
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

  // Only refresh if explicitly requested (e.g., after sync completes)
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
      
      // If this is the current session, stop polling
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
  syncResults.value = null; // Clear previous results

  try {
    // Use the real Microsoft Graph integration endpoint
    const response = await apiClient.post("/integrations/graph/sync", {
      entities: selectedEntities.value,
      use_delta: useDelta,
    });

    showSyncModal.value = false;

    if (response.data.success && response.data.session_id) {
      successMessage.value = response.data.message || "Sync started successfully";
      
      // Start progress polling
      startProgressPolling(response.data.session_id);
      
      // Log session details for debugging
      console.log("Sync started with session ID:", response.data.session_id);
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

// Format status for display
const getStatusDisplay = (status: string) => {
  switch (status) {
    case "connected":
      return {
        text: "Connected",
        class: "bg-status-success/20 text-status-success border-status-success/50",
      };
    case "disconnected":
      return {
        text: "Not Connected",
        class: "bg-surface-alt text-tertiary border-default",
      };
    case "connecting":
      return {
        text: "Connecting...",
        class: "bg-accent/20 text-accent border-accent",
      };
    case "error":
      return {
        text: "Connection Error",
        class: "bg-status-error/20 text-status-error border-status-error/50",
      };
    default:
      return {
        text: "Unknown",
        class: "bg-surface-alt text-tertiary border-default",
      };
  }
};

// Format sync type for display
const formatSyncType = (syncType?: string): string => {
  const type = syncType ?? 'unknown';

  switch (type) {
    case 'users':
      return 'User Accounts';
    case 'profile_photos':
      return 'Profile Photos';
    case 'devices':
      return 'Managed Devices';
    case 'groups':
      return 'Security Groups';
    default:
      return type.charAt(0).toUpperCase() + type.slice(1);
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

// Fetch active syncs
const fetchActiveSyncs = async () => {
  isLoadingActiveSyncs.value = true;
  try {
    const response = await apiClient.get("/integrations/graph/active-syncs");
    activeSyncs.value = response.data.active_syncs || [];
    
    // Only start monitoring if there are truly active syncs (running/starting) and not already monitoring
    if (activeSyncs.value.length > 0 && !isSyncing.value) {
      const runningSyncs = activeSyncs.value.filter(sync => 
        sync.status === 'running' || sync.status === 'starting'
      );
      
      if (runningSyncs.length > 0) {
        const activeSync = runningSyncs[0];
      console.log("Found active sync, resuming monitoring:", activeSync.session_id);
      startProgressPolling(activeSync.session_id);
      }
    }
  } catch (error) {
    console.error("Failed to fetch active syncs:", error);
    // Don't show error message for this as it's a background operation
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
    lastSyncDetails.value = response.data;
  } catch (error) {
    console.error("Failed to fetch last sync details:", error);
    // Don't show error message for this as it's a background operation
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
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-6 flex justify-between items-center">
      <BackButton
        fallbackRoute="/admin/data-import"
        label="Back to Data Import"
      />

      <!-- Sync button - prominent position -->
      <button
        @click="syncData"
        :disabled="connectionStatus !== 'connected' || isLoading || isSyncing"
        class="px-4 py-2 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
      >
        <svg v-if="isSyncing" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
          <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
          <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
        </svg>
        <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
        </svg>
        {{ isSyncing ? 'Syncing...' : 'Sync Data' }}
      </button>
    </div>

    <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-6">
        <h1 class="text-2xl font-bold text-primary">
          Microsoft Graph Connection
        </h1>
        <p class="text-secondary mt-2">
          Validate your Microsoft Graph API configuration and manage data synchronization from Microsoft services
        </p>
      </div>

      <!-- Configuration Notice -->
      <EnvConfigNotice>
        Microsoft Graph integration is configured through environment variables.
        Set <code class="bg-surface px-1 rounded text-primary">MICROSOFT_CLIENT_ID</code>,
        <code class="bg-surface px-1 rounded text-primary">MICROSOFT_CLIENT_SECRET</code>, and
        <code class="bg-surface px-1 rounded text-primary">MICROSOFT_TENANT_ID</code> in your environment.
      </EnvConfigNotice>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Error message -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Connection status card -->
      <div class="bg-surface border border-default rounded-xl hover:border-strong transition-colors">
        <div class="p-4 flex flex-col gap-3">
          <!-- Header row with icon -->
          <div class="flex items-center gap-3">
            <!-- Microsoft icon -->
            <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-accent/20 flex items-center justify-center text-accent">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" v-html="AdminIcons.microsoft"></svg>
            </div>

            <!-- Title and status badges -->
            <div class="flex-1 flex items-center gap-2 flex-wrap">
              <span class="font-medium text-primary">Connection Status</span>
              <span
                class="px-1.5 py-0.5 text-xs rounded-full border"
                :class="{
                  'bg-status-success/20 text-status-success border-status-success/50': connectionStatus === 'connected',
                  'bg-surface-alt text-tertiary border-default': connectionStatus === 'disconnected',
                  'bg-accent/20 text-accent border-accent/50': connectionStatus === 'connecting',
                  'bg-status-error/20 text-status-error border-status-error/50': connectionStatus === 'error'
                }"
              >
                {{ getStatusDisplay(connectionStatus).text }}
              </span>
            </div>
          </div>

          <!-- Last sync info -->
          <div v-if="lastSync" class="text-sm">
            <span class="text-tertiary">Last synchronized:</span> <span class="text-primary">{{ lastSync }}</span>
          </div>
        </div>
      </div>

      <!-- Configuration Details -->
      <div
        v-if="configValidation"
        class="bg-surface border border-default rounded-xl hover:border-strong transition-colors"
      >
        <div class="p-4 flex flex-col gap-3">
          <!-- Header row with icon -->
          <div class="flex items-center gap-3">
            <!-- Config icon -->
            <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-surface-alt flex items-center justify-center text-tertiary">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" v-html="AdminIcons.cog"></svg>
            </div>

            <!-- Title and status -->
            <div class="flex-1 flex items-center gap-2 flex-wrap">
              <span class="font-medium text-primary">Configuration Details</span>
              <span
                class="px-1.5 py-0.5 text-xs rounded-full border"
                :class="configValidation.valid
                  ? 'bg-status-success/20 text-status-success border-status-success/50'
                  : 'bg-status-error/20 text-status-error border-status-error/50'"
              >
                {{ configValidation.valid ? 'Valid' : 'Invalid' }}
              </span>
            </div>
          </div>

          <!-- Configuration values - two column layout -->
          <div class="flex flex-col md:flex-row gap-4 text-sm">
            <!-- Left: Client ID and Tenant ID (full values) -->
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
            <!-- Right: Secret and Status -->
            <div class="flex flex-row md:flex-col gap-4 md:gap-2 md:w-28 md:flex-shrink-0">
              <div class="flex flex-col gap-0.5">
                <span class="text-tertiary text-xs">Secret</span>
                <span :class="configValidation.client_secret_configured ? 'text-status-success' : 'text-status-error'" class="font-medium bg-surface-alt px-2 py-1.5 rounded text-xs">{{ configValidation.client_secret_configured ? 'Configured' : 'Not Set' }}</span>
              </div>
              <div class="flex flex-col gap-0.5">
                <span class="text-tertiary text-xs">Status</span>
                <span :class="configValidation.valid ? 'text-status-success' : 'text-status-error'" class="font-medium bg-surface-alt px-2 py-1.5 rounded text-xs">{{ configValidation.valid ? 'Ready' : 'Incomplete' }}</span>
              </div>
            </div>
          </div>

          <!-- Configuration Issues -->
          <div
            v-if="configValidation.issues && configValidation.issues.length > 0"
            class="p-2 bg-status-error/10 border border-status-error/30 rounded-lg text-sm text-status-error flex items-start gap-2"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 flex-shrink-0 mt-0.5" viewBox="0 0 20 20" fill="currentColor">
              <path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7 4a1 1 0 11-2 0 1 1 0 012 0zm-1-9a1 1 0 00-1 1v4a1 1 0 102 0V6a1 1 0 00-1-1z" clip-rule="evenodd" />
            </svg>
            <div>
              <span v-for="(issue, index) in configValidation.issues" :key="issue">
                {{ issue }}<span v-if="index < configValidation.issues.length - 1">; </span>
              </span>
            </div>
          </div>
        </div>
      </div>

      <!-- Available entities -->
      <div class="bg-surface border border-default rounded-xl hover:border-strong transition-colors">
        <div class="p-4 flex flex-col gap-3">
          <!-- Header row with icon -->
          <div class="flex items-center gap-3">
            <!-- Database icon -->
            <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-status-success/20 flex items-center justify-center text-status-success">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" v-html="AdminIcons.database"></svg>
            </div>

            <!-- Title -->
            <div class="flex-1">
              <span class="font-medium text-primary">Available Data Entities</span>
            </div>
          </div>

          <!-- Entity selection grid -->
          <div class="grid grid-cols-1 md:grid-cols-3 gap-3">
            <div
              v-for="entity in availableEntities"
              :key="entity.id"
              @click="toggleEntity(entity.id)"
              class="p-3 rounded-lg border cursor-pointer transition-colors"
              :class="selectedEntities.includes(entity.id)
                ? 'bg-accent/10 border-accent/50'
                : 'bg-surface-alt border-default hover:border-strong'"
            >
              <div class="flex items-center justify-between mb-1">
                <h3 class="text-primary font-medium text-sm">{{ entity.name }}</h3>
                <span
                  class="px-1.5 py-0.5 text-xs rounded-full border"
                  :class="selectedEntities.includes(entity.id)
                    ? 'bg-accent/20 text-accent border-accent/50'
                    : 'bg-surface-alt text-tertiary border-default'"
                >
                  {{ selectedEntities.includes(entity.id) ? 'Selected' : 'Select' }}
                </span>
              </div>
              <p class="text-xs text-secondary">{{ entity.description }}</p>
            </div>
          </div>
        </div>
      </div>

      <!-- Sync Progress / Active Synchronizations -->
      <div
        v-if="(isSyncing && syncProgress) || (activeSyncs.length > 0 && !isSyncing) || isLoadingActiveSyncs"
        class="bg-surface border border-default rounded-xl hover:border-strong transition-colors"
      >
        <!-- Show real-time progress when actively monitoring a sync -->
        <div v-if="isSyncing && syncProgress" class="p-4 flex flex-col gap-3">
          <!-- Header row with icon -->
          <div class="flex items-center gap-3">
            <!-- Sync icon -->
            <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-accent/20 flex items-center justify-center text-accent">
              <svg class="animate-spin h-5 w-5" fill="none" viewBox="0 0 24 24">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
            </div>

            <!-- Title and status -->
            <div class="flex-1 flex items-center gap-2 flex-wrap">
              <span class="font-medium text-primary">{{ formatSyncType(syncProgress.sync_type || syncProgress.entity) }}</span>
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

            <!-- Cancel button -->
            <button
              v-if="(syncProgress.status === 'running' || syncProgress.status === 'starting') && currentSessionId"
              @click="cancelSync(currentSessionId!)"
              class="px-3 py-1.5 bg-status-error/20 text-status-error border border-status-error/50 rounded-lg text-sm hover:bg-status-error/30 font-medium transition-colors flex items-center gap-1.5 whitespace-nowrap"
            >
              Cancel
            </button>
          </div>

          <!-- Progress info -->
          <div class="flex items-center justify-between text-sm">
            <span class="text-secondary">{{ syncProgress.message }}</span>
            <span class="text-primary font-medium">{{ syncProgress.current }} / {{ syncProgress.total }} <span v-if="syncProgress.total > 0" class="text-tertiary">({{ Math.round((syncProgress.current / syncProgress.total) * 100) }}%)</span></span>
          </div>

          <!-- Progress Bar -->
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
                width: syncProgress.total > 0
                  ? `${Math.round((syncProgress.current / syncProgress.total) * 100)}%`
                  : (syncProgress.status === 'completed' || syncProgress.status === 'completed_with_errors') ? '100%' : '0%'
              }"
            ></div>
          </div>
        </div>

        <!-- Show active syncs when not actively monitoring -->
        <div v-else class="p-4 flex flex-col gap-3">
          <!-- Header row with icon -->
          <div class="flex items-center gap-3">
            <!-- Sync icon -->
            <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-accent/20 flex items-center justify-center text-accent">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
            </div>

            <!-- Title -->
            <div class="flex-1">
              <span class="font-medium text-primary">Active Synchronizations</span>
            </div>

            <!-- Refresh button -->
            <button
              @click="fetchActiveSyncs"
              :disabled="isLoadingActiveSyncs"
              class="px-3 py-1.5 bg-surface-alt text-primary border border-default rounded-lg text-sm hover:bg-surface-hover font-medium transition-colors disabled:opacity-50 flex items-center gap-1.5"
            >
              <svg v-if="isLoadingActiveSyncs" class="animate-spin h-3.5 w-3.5" fill="none" viewBox="0 0 24 24">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
              <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
              Refresh
            </button>
          </div>

          <div v-if="isLoadingActiveSyncs" class="text-center py-4 text-secondary text-sm">
            Loading active syncs...
          </div>

          <div v-else-if="activeSyncs.length === 0" class="text-center py-4 text-secondary text-sm">
            No active synchronizations
          </div>

          <div v-else class="flex flex-col gap-2">
            <div
              v-for="sync in activeSyncs"
              :key="sync.session_id"
              class="p-3 bg-surface-alt rounded-lg border border-default"
            >
              <div class="flex items-center gap-3">
                <div class="flex-1">
                  <div class="flex items-center gap-2 mb-1">
                    <span class="font-medium text-primary text-sm">{{ formatSyncType(sync.sync_type) }}</span>
                    <span
                      class="px-1.5 py-0.5 text-xs rounded-full border"
                      :class="{
                        'bg-accent/20 text-accent border-accent/50': sync.status === 'running' || sync.status === 'starting',
                        'bg-status-warning/20 text-status-warning border-status-warning/50': sync.status === 'cancelling' || sync.status === 'completed_with_errors',
                        'bg-status-success/20 text-status-success border-status-success/50': sync.status === 'completed',
                        'bg-status-error/20 text-status-error border-status-error/50': sync.status === 'error'
                      }"
                    >
                      {{ sync.status === 'completed_with_errors' ? 'Completed with errors' : sync.status }}
                    </span>
                  </div>
                  <div class="text-xs text-secondary">{{ sync.message }}</div>
                  <div class="flex items-center gap-3 text-xs text-tertiary mt-1">
                    <span>{{ formatTimeAgo(sync.started_at) }}</span>
                    <span>{{ sync.current }} / {{ sync.total }}<span v-if="sync.total > 0"> ({{ Math.round((sync.current / sync.total) * 100) }}%)</span></span>
                  </div>
                  <!-- Progress bar -->
                  <div class="w-full bg-surface rounded-full h-1.5 mt-2">
                    <div
                      class="h-1.5 rounded-full transition-all duration-300"
                      :class="{
                        'bg-accent': sync.status === 'running' || sync.status === 'starting',
                        'bg-status-warning': sync.status === 'cancelling' || sync.status === 'completed_with_errors',
                        'bg-status-success': sync.status === 'completed',
                        'bg-status-error': sync.status === 'error'
                      }"
                      :style="{
                        width: sync.total > 0
                          ? `${Math.round((sync.current / sync.total) * 100)}%`
                          : (sync.status === 'completed' || sync.status === 'completed_with_errors') ? '100%' : '0%'
                      }"
                    ></div>
                  </div>
                </div>

                <div class="flex gap-2">
                  <button
                    v-if="currentSessionId !== sync.session_id && (sync.status === 'running' || sync.status === 'starting')"
                    @click="resumeSync(sync.session_id)"
                    class="px-2 py-1 bg-accent/20 text-accent border border-accent/50 rounded text-xs hover:bg-accent/30 transition-colors"
                  >
                    Monitor
                  </button>
                  <button
                    v-if="sync.status === 'running' || sync.status === 'starting'"
                    @click="cancelSync(sync.session_id)"
                    class="px-2 py-1 bg-status-error/20 text-status-error border border-status-error/50 rounded text-xs hover:bg-status-error/30 transition-colors"
                  >
                    Cancel
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Last Sync Details -->
      <div
        v-if="lastSyncDetails || isLoadingLastSync"
        class="bg-surface border border-default rounded-xl hover:border-strong transition-colors"
      >
        <div class="p-4 flex flex-col gap-3">
          <!-- Header row with icon -->
          <div class="flex items-center gap-3">
            <!-- History icon -->
            <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-accent/20 flex items-center justify-center text-accent">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
            </div>

            <!-- Title and badges -->
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

            <!-- Refresh button -->
            <button
              @click="fetchLastSyncDetails"
              :disabled="isLoadingLastSync"
              class="px-3 py-1.5 bg-surface-alt text-secondary border border-default rounded-lg text-sm hover:bg-surface-hover hover:text-primary font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-1.5 whitespace-nowrap"
            >
              <svg :class="{ 'animate-spin': isLoadingLastSync }" xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
              Refresh
            </button>
          </div>

          <!-- Loading state -->
          <div v-if="isLoadingLastSync" class="flex items-center justify-center py-4 text-secondary">
            <svg class="animate-spin h-5 w-5 mr-2" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
            Loading sync details...
          </div>

          <!-- Sync Details Content -->
          <div v-else-if="lastSyncDetails">
            <!-- Compact stats row -->
            <div class="grid grid-cols-2 md:grid-cols-4 gap-x-4 gap-y-1 text-sm">
              <div><span class="text-tertiary">Type:</span> <span class="text-primary">{{ lastSyncDetails.is_delta ? 'Delta' : 'Full' }}</span></div>
              <div><span class="text-tertiary">Started:</span> <span class="text-primary">{{ formatTimeAgo(lastSyncDetails.started_at) }}</span></div>
              <div><span class="text-tertiary">Duration:</span> <span class="text-primary">{{ formatDuration(lastSyncDetails.started_at, lastSyncDetails.updated_at) }}</span></div>
              <div><span class="text-tertiary">Progress:</span> <span class="text-primary"><span v-if="lastSyncDetails.total > 0">{{ lastSyncDetails.current }} / {{ lastSyncDetails.total }}</span><span v-else-if="lastSyncDetails.status === 'cancelled'">Cancelled</span><span v-else-if="lastSyncDetails.status === 'error'">Failed</span><span v-else>-</span></span></div>
            </div>

            <!-- Progress Bar -->
            <div class="w-full bg-surface-alt rounded-full h-2 mt-3">
              <div
                class="h-2 rounded-full transition-all duration-300"
                :class="{
                  'bg-status-success': lastSyncDetails.status === 'completed',
                  'bg-status-warning': lastSyncDetails.status === 'completed_with_errors',
                  'bg-status-error': lastSyncDetails.status === 'error',
                  'bg-tertiary': lastSyncDetails.status === 'cancelled'
                }"
                :style="{
                  width: lastSyncDetails.total > 0
                    ? `${Math.round((lastSyncDetails.current / lastSyncDetails.total) * 100)}%`
                    : (lastSyncDetails.status === 'completed' || lastSyncDetails.status === 'completed_with_errors') ? '100%' : '0%'
                }"
              ></div>
            </div>

            <!-- Message -->
            <div v-if="lastSyncDetails.message" class="text-sm text-secondary mt-2">
              {{ lastSyncDetails.message }}
            </div>
          </div>

          <!-- No sync history -->
          <div v-else class="text-center py-6 text-secondary">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-10 w-10 mx-auto mb-2 opacity-50" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <p class="font-medium">No Previous Synchronizations</p>
            <p class="text-sm mt-1 text-tertiary">Run your first sync to see details here</p>
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
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 flex-shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
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
                  <svg v-if="result.status === 'completed'" class="w-4 h-4 text-status-success" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path>
                  </svg>
                  <svg v-else-if="result.status === 'completed_with_errors'" class="w-4 h-4 text-status-warning" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.864-.833-2.634 0l-5.898 8.5c-.77.833.192 2.5 1.732 2.5z"></path>
                  </svg>
                  <svg v-else class="w-4 h-4 text-status-error" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path>
                  </svg>
                </div>
              </div>

              <!-- Error Details -->
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

          <!-- Overall Summary -->
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
            <svg v-if="isLoading || isSyncing" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
            <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
            </svg>
            {{ isLoading ? 'Starting...' : isSyncing ? 'Syncing...' : 'Start Sync' }}
          </button>
        </div>
      </div>
    </Modal>
  </div>
</template>