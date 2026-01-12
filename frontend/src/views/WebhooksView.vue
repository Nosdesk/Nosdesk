<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import BackButton from '@/components/common/BackButton.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Modal from '@/components/Modal.vue';
import webhookService from '@/services/webhookService';
import { formatDistanceToNow } from 'date-fns';
import type {
  Webhook,
  WebhookCreated,
  CreateWebhookRequest,
  UpdateWebhookRequest,
  WebhookDelivery,
} from '@/types/webhook';
import { WEBHOOK_EVENT_CATEGORIES } from '@/types/webhook';

// State
const isLoading = ref(false);
const isSaving = ref(false);
const errorMessage = ref('');
const successMessage = ref('');
const webhooks = ref<Webhook[]>([]);

// Modal states
const showCreateModal = ref(false);
const showEditModal = ref(false);
const showDeleteConfirm = ref(false);
const showSecretCreated = ref(false);
const showDeliveries = ref(false);
const showRegenerateConfirm = ref(false);
const webhookToDelete = ref<Webhook | null>(null);
const webhookToEdit = ref<Webhook | null>(null);
const webhookForDeliveries = ref<Webhook | null>(null);
const createdWebhook = ref<WebhookCreated | null>(null);
const copiedSecret = ref(false);
const deliveries = ref<WebhookDelivery[]>([]);
const isLoadingDeliveries = ref(false);
const newSecret = ref<string | null>(null);

// Form state
const createForm = ref<CreateWebhookRequest>({
  name: '',
  url: '',
  events: [],
  headers: {},
});

const editForm = ref<UpdateWebhookRequest>({
  name: '',
  url: '',
  events: [],
  enabled: true,
  headers: {},
});

// Custom headers for create form
const customHeaders = ref<{ key: string; value: string }[]>([]);

// Custom headers for edit form
const editCustomHeaders = ref<{ key: string; value: string }[]>([]);

// Computed - categorized webhooks
const enabledWebhooks = computed(() =>
  webhooks.value.filter(w => w.enabled)
);

const disabledWebhooks = computed(() =>
  webhooks.value.filter(w => !w.enabled)
);

// =============================================================================
// Helper Functions (DRY)
// =============================================================================

// Format date helper
const formatDate = (dateStr: string | null) => {
  if (!dateStr) return 'Never';
  try {
    return formatDistanceToNow(new Date(dateStr), { addSuffix: true });
  } catch {
    return dateStr;
  }
};

// Extract error message from axios error
const getErrorMessage = (error: unknown, defaultMsg: string): string => {
  const axiosError = error as { response?: { data?: string } };
  return axiosError.response?.data || defaultMsg;
};

// Convert headers object to array format
const objectToHeaders = (obj: Record<string, string> | null): { key: string; value: string }[] => {
  return obj ? Object.entries(obj).map(([key, value]) => ({ key, value })) : [];
};

// Get webhook status
const getWebhookStatus = (webhook: Webhook) => {
  if (!webhook.enabled) {
    return { label: 'Disabled', color: 'text-secondary', bg: 'bg-surface-alt' };
  }
  if (webhook.failure_count >= 5) {
    return { label: 'Failing', color: 'text-status-error', bg: 'bg-status-error/10' };
  }
  if (webhook.failure_count > 0) {
    return { label: 'Warning', color: 'text-status-warning', bg: 'bg-status-warning/10' };
  }
  return { label: 'Active', color: 'text-status-success', bg: 'bg-status-success/10' };
};

// Load webhooks
const loadWebhooks = async () => {
  isLoading.value = true;
  errorMessage.value = '';

  try {
    const result = await webhookService.listWebhooks();
    webhooks.value = Array.isArray(result) ? result : [];
  } catch (error) {
    console.error('Failed to load webhooks:', error);
    errorMessage.value = getErrorMessage(error, 'Failed to load webhooks');
    webhooks.value = [];
  } finally {
    isLoading.value = false;
  }
};

// Open create modal
const openCreateModal = () => {
  createForm.value = {
    name: '',
    url: '',
    events: [],
    headers: {},
  };
  customHeaders.value = [];
  showCreateModal.value = true;
};

// Toggle event selection
const toggleEvent = (eventValue: string, formEvents: string[]) => {
  const index = formEvents.indexOf(eventValue);
  if (index === -1) {
    formEvents.push(eventValue);
  } else {
    formEvents.splice(index, 1);
  }
};

// Toggle all events in a category
const toggleCategory = (category: string, formEvents: string[], isCreate: boolean) => {
  const categoryEvents = WEBHOOK_EVENT_CATEGORIES[category] || [];
  const allSelected = categoryEvents.every(e => formEvents.includes(e.value));

  if (allSelected) {
    // Remove all events from this category
    categoryEvents.forEach(e => {
      const index = formEvents.indexOf(e.value);
      if (index !== -1) formEvents.splice(index, 1);
    });
  } else {
    // Add all events from this category
    categoryEvents.forEach(e => {
      if (!formEvents.includes(e.value)) {
        formEvents.push(e.value);
      }
    });
  }

  // Update the form
  if (isCreate) {
    createForm.value.events = [...formEvents];
  } else {
    editForm.value.events = [...formEvents];
  }
};

// Add custom header
const addHeader = (isCreate: boolean) => {
  if (isCreate) {
    customHeaders.value.push({ key: '', value: '' });
  } else {
    editCustomHeaders.value.push({ key: '', value: '' });
  }
};

// Remove custom header
const removeHeader = (index: number, isCreate: boolean) => {
  if (isCreate) {
    customHeaders.value.splice(index, 1);
  } else {
    editCustomHeaders.value.splice(index, 1);
  }
};

// Convert headers array to object
const headersToObject = (headers: { key: string; value: string }[]): Record<string, string> => {
  const obj: Record<string, string> = {};
  headers.forEach(h => {
    if (h.key.trim()) {
      obj[h.key.trim()] = h.value;
    }
  });
  return obj;
};

// Create webhook
const createWebhook = async () => {
  if (!createForm.value.name.trim()) {
    errorMessage.value = 'Webhook name is required';
    return;
  }
  if (!createForm.value.url.trim()) {
    errorMessage.value = 'URL is required';
    return;
  }
  if (createForm.value.events.length === 0) {
    errorMessage.value = 'At least one event must be selected';
    return;
  }

  isSaving.value = true;
  errorMessage.value = '';

  try {
    const request: CreateWebhookRequest = {
      name: createForm.value.name.trim(),
      url: createForm.value.url.trim(),
      events: createForm.value.events,
      headers: customHeaders.value.length > 0 ? headersToObject(customHeaders.value) : undefined,
    };

    const result = await webhookService.createWebhook(request);
    createdWebhook.value = result;
    showCreateModal.value = false;
    showSecretCreated.value = true;
    copiedSecret.value = false;
    await loadWebhooks();
  } catch (error) {
    errorMessage.value = getErrorMessage(error, 'Failed to create webhook');
  } finally {
    isSaving.value = false;
  }
};

// Copy secret to clipboard
const copySecret = async (secret: string) => {
  try {
    await navigator.clipboard.writeText(secret);
    copiedSecret.value = true;
    setTimeout(() => copiedSecret.value = false, 2000);
  } catch (error) {
    console.error('Failed to copy secret:', error);
  }
};

// Open edit modal
const openEditModal = (webhook: Webhook) => {
  webhookToEdit.value = webhook;
  editForm.value = {
    name: webhook.name,
    url: webhook.url,
    events: [...webhook.events],
    enabled: webhook.enabled,
    headers: webhook.headers || {},
  };
  // Convert headers object to array
  editCustomHeaders.value = objectToHeaders(webhook.headers);
  showEditModal.value = true;
};

// Update webhook
const updateWebhook = async () => {
  if (!webhookToEdit.value) return;

  if (!editForm.value.name?.trim()) {
    errorMessage.value = 'Webhook name is required';
    return;
  }
  if (!editForm.value.url?.trim()) {
    errorMessage.value = 'URL is required';
    return;
  }
  if (!editForm.value.events || editForm.value.events.length === 0) {
    errorMessage.value = 'At least one event must be selected';
    return;
  }

  isSaving.value = true;
  errorMessage.value = '';

  try {
    const request: UpdateWebhookRequest = {
      name: editForm.value.name?.trim(),
      url: editForm.value.url?.trim(),
      events: editForm.value.events,
      enabled: editForm.value.enabled,
      headers: editCustomHeaders.value.length > 0 ? headersToObject(editCustomHeaders.value) : {},
    };

    await webhookService.updateWebhook(webhookToEdit.value.uuid, request);
    successMessage.value = 'Webhook updated successfully';
    showEditModal.value = false;
    webhookToEdit.value = null;
    await loadWebhooks();

    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    errorMessage.value = getErrorMessage(error, 'Failed to update webhook');
  } finally {
    isSaving.value = false;
  }
};

// Confirm delete
const confirmDelete = (webhook: Webhook) => {
  webhookToDelete.value = webhook;
  showDeleteConfirm.value = true;
};

// Delete webhook
const deleteWebhook = async () => {
  if (!webhookToDelete.value) return;

  isSaving.value = true;
  errorMessage.value = '';

  try {
    await webhookService.deleteWebhook(webhookToDelete.value.uuid);
    successMessage.value = 'Webhook deleted successfully';
    showDeleteConfirm.value = false;
    webhookToDelete.value = null;
    await loadWebhooks();

    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    errorMessage.value = getErrorMessage(error, 'Failed to delete webhook');
  } finally {
    isSaving.value = false;
  }
};

// View deliveries
const viewDeliveries = async (webhook: Webhook) => {
  webhookForDeliveries.value = webhook;
  deliveries.value = [];
  isLoadingDeliveries.value = true;
  showDeliveries.value = true;

  try {
    deliveries.value = await webhookService.getDeliveries(webhook.uuid);
  } catch (error) {
    console.error('Failed to load deliveries:', error);
  } finally {
    isLoadingDeliveries.value = false;
  }
};

// Test webhook
const testWebhook = async (webhook: Webhook) => {
  isSaving.value = true;
  errorMessage.value = '';

  try {
    await webhookService.testWebhook(webhook.uuid);
    successMessage.value = 'Test event sent to webhook';
    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    errorMessage.value = getErrorMessage(error, 'Failed to send test event');
  } finally {
    isSaving.value = false;
  }
};

// Confirm regenerate secret
const confirmRegenerateSecret = () => {
  showRegenerateConfirm.value = true;
};

// Regenerate secret
const regenerateSecret = async () => {
  if (!webhookToEdit.value) return;

  isSaving.value = true;
  errorMessage.value = '';

  try {
    const result = await webhookService.updateWebhook(webhookToEdit.value.uuid, {
      regenerate_secret: true,
    });
    // The updated webhook will have the new secret preview
    // We need to show the user the full secret from a special response
    // For now, show success and close the confirm modal
    newSecret.value = null; // Backend doesn't return the new secret on regenerate via update
    successMessage.value = 'Secret regenerated - check webhook deliveries for the new signature';
    showRegenerateConfirm.value = false;
    await loadWebhooks();
    // Refresh the edit form with updated data
    if (webhookToEdit.value) {
      const updated = webhooks.value.find(w => w.uuid === webhookToEdit.value?.uuid);
      if (updated) {
        webhookToEdit.value = updated;
        editForm.value.enabled = updated.enabled;
      }
    }
    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    errorMessage.value = getErrorMessage(error, 'Failed to regenerate secret');
  } finally {
    isSaving.value = false;
  }
};

// Get delivery status color
const getDeliveryStatusColor = (delivery: WebhookDelivery) => {
  if (delivery.error_message) {
    return 'text-status-error bg-status-error/10';
  }
  if (delivery.response_status && delivery.response_status >= 200 && delivery.response_status < 300) {
    return 'text-status-success bg-status-success/10';
  }
  if (delivery.response_status && delivery.response_status >= 400) {
    return 'text-status-error bg-status-error/10';
  }
  return 'text-status-warning bg-status-warning/10';
};

onMounted(() => {
  loadWebhooks();
});
</script>

<template>
  <div class="flex-1">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-4 sm:px-6 flex flex-col sm:flex-row justify-between items-start sm:items-center gap-3 sm:gap-4">
      <BackButton fallbackRoute="/admin/settings" label="Back to Administration" />
      <button
        @click="openCreateModal"
        class="px-3 py-1.5 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors flex items-center gap-1.5"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6m0 0v6m0-6h6m-6 0H6" />
        </svg>
        <span class="hidden xs:inline">Create Webhook</span>
        <span class="xs:hidden">Create</span>
      </button>
    </div>

    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">Webhooks</h1>
        <p class="text-secondary text-sm sm:text-base mt-1">Manage webhooks for external integrations</p>
      </div>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Error message -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Loading state -->
      <LoadingSpinner v-if="isLoading" text="Loading webhooks..." />

      <!-- Webhooks list -->
      <div v-else class="flex flex-col gap-4">
        <!-- Active webhooks -->
        <div v-if="enabledWebhooks.length > 0" class="flex flex-col gap-2 sm:gap-3">
          <h2 class="text-sm font-medium text-secondary uppercase tracking-wide">Active Webhooks</h2>
          <div
            v-for="webhook in enabledWebhooks"
            :key="webhook.uuid"
            class="bg-surface border border-default rounded-lg sm:rounded-xl"
          >
            <div class="p-3 sm:p-4 flex items-start gap-3 sm:gap-4">
              <!-- Webhook icon -->
              <div class="w-8 h-8 sm:w-10 sm:h-10 rounded-lg flex items-center justify-center flex-shrink-0"
                   :class="getWebhookStatus(webhook).bg">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 sm:h-5 sm:w-5" :class="getWebhookStatus(webhook).color" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1" />
                </svg>
              </div>

              <!-- Webhook info -->
              <div class="flex-1 min-w-0">
                <div class="flex flex-col sm:flex-row sm:items-center gap-1 sm:gap-2">
                  <h3 class="font-medium text-primary text-sm sm:text-base truncate">{{ webhook.name }}</h3>
                  <span class="px-1.5 py-0.5 text-xs rounded font-medium"
                        :class="[getWebhookStatus(webhook).color, getWebhookStatus(webhook).bg]">
                    {{ getWebhookStatus(webhook).label }}
                  </span>
                  <span v-if="webhook.failure_count > 0" class="px-1.5 py-0.5 text-xs bg-status-error/10 text-status-error rounded">
                    {{ webhook.failure_count }} failures
                  </span>
                </div>
                <div class="text-xs text-secondary mt-1 truncate font-mono">{{ webhook.url }}</div>
                <div class="flex flex-wrap items-center gap-2 mt-1 text-xs text-secondary">
                  <span>Secret: <code class="px-1 py-0.5 bg-surface-alt rounded">{{ webhook.secret_preview }}</code></span>
                  <span class="text-tertiary">|</span>
                  <span>{{ webhook.events.length }} event{{ webhook.events.length === 1 ? '' : 's' }}</span>
                  <span class="text-tertiary">|</span>
                  <span>Last triggered: {{ formatDate(webhook.last_triggered_at) }}</span>
                </div>
                <div v-if="webhook.disabled_reason" class="text-xs text-status-error mt-1">
                  {{ webhook.disabled_reason }}
                </div>
              </div>

              <!-- Actions -->
              <div class="flex-shrink-0 flex items-center gap-1">
                <button
                  @click="testWebhook(webhook)"
                  class="p-1.5 sm:p-2 text-secondary hover:text-accent hover:bg-accent/10 rounded-md sm:rounded-lg transition-colors"
                  title="Send test event"
                  :disabled="isSaving"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832l3.197-2.132a1 1 0 000-1.664z" />
                    <path stroke-linecap="round" stroke-linejoin="round" d="M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                  </svg>
                </button>
                <button
                  @click="viewDeliveries(webhook)"
                  class="p-1.5 sm:p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-md sm:rounded-lg transition-colors"
                  title="View deliveries"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-3 7h3m-3 4h3m-6-4h.01M9 16h.01" />
                  </svg>
                </button>
                <button
                  @click="openEditModal(webhook)"
                  class="p-1.5 sm:p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-md sm:rounded-lg transition-colors"
                  title="Edit webhook"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
                  </svg>
                </button>
                <button
                  @click="confirmDelete(webhook)"
                  class="p-1.5 sm:p-2 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-md sm:rounded-lg transition-colors"
                  title="Delete webhook"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                  </svg>
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- Disabled webhooks -->
        <div v-if="disabledWebhooks.length > 0" class="flex flex-col gap-2 sm:gap-3 mt-4">
          <h2 class="text-sm font-medium text-secondary uppercase tracking-wide">Disabled Webhooks</h2>
          <div
            v-for="webhook in disabledWebhooks"
            :key="webhook.uuid"
            class="bg-surface border border-default rounded-lg sm:rounded-xl opacity-60"
          >
            <div class="p-3 sm:p-4 flex items-start gap-3 sm:gap-4">
              <!-- Webhook icon -->
              <div class="w-8 h-8 sm:w-10 sm:h-10 rounded-lg bg-surface-alt flex items-center justify-center flex-shrink-0">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 sm:h-5 sm:w-5 text-secondary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1" />
                </svg>
              </div>

              <!-- Webhook info -->
              <div class="flex-1 min-w-0">
                <div class="flex flex-col sm:flex-row sm:items-center gap-1 sm:gap-2">
                  <h3 class="font-medium text-secondary text-sm sm:text-base truncate">{{ webhook.name }}</h3>
                  <span class="px-1.5 py-0.5 text-xs bg-surface-alt text-secondary rounded">Disabled</span>
                </div>
                <div class="text-xs text-tertiary mt-1 truncate font-mono">{{ webhook.url }}</div>
                <div v-if="webhook.disabled_reason" class="text-xs text-status-error mt-1">
                  {{ webhook.disabled_reason }}
                </div>
              </div>

              <!-- Actions -->
              <div class="flex-shrink-0 flex items-center gap-1">
                <button
                  @click="openEditModal(webhook)"
                  class="p-1.5 sm:p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-md sm:rounded-lg transition-colors"
                  title="Edit webhook"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
                  </svg>
                </button>
                <button
                  @click="confirmDelete(webhook)"
                  class="p-1.5 sm:p-2 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-md sm:rounded-lg transition-colors"
                  title="Delete webhook"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                  </svg>
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- Empty state -->
        <EmptyState
          v-if="webhooks.length === 0 && !isLoading"
          icon="link"
          title="No webhooks"
          description="Create a webhook to send events to external services"
          action-label="Create Webhook"
          variant="card"
          @action="openCreateModal"
        />
      </div>
    </div>

    <!-- Create Webhook Modal -->
    <Modal
      :show="showCreateModal"
      title="Create Webhook"
      size="lg"
      @close="showCreateModal = false"
    >
      <form @submit.prevent="createWebhook" class="flex flex-col gap-4">
        <!-- Name -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">Name</label>
          <input
            v-model="createForm.name"
            type="text"
            placeholder="e.g., Slack Notifications"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            required
          />
        </div>

        <!-- URL -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">Payload URL</label>
          <input
            v-model="createForm.url"
            type="url"
            placeholder="https://example.com/webhook"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent font-mono text-sm"
            required
          />
          <p class="text-xs text-tertiary mt-1">POST requests will be sent to this URL</p>
        </div>

        <!-- Events -->
        <div>
          <label class="block text-sm font-medium text-primary mb-2">Events</label>
          <div class="border border-default rounded-lg max-h-64 overflow-y-auto">
            <div v-for="(events, category) in WEBHOOK_EVENT_CATEGORIES" :key="category" class="border-b border-default last:border-b-0">
              <div
                class="px-3 py-2 bg-surface-alt flex items-center justify-between cursor-pointer hover:bg-surface-hover"
                @click="toggleCategory(category, createForm.events, true)"
              >
                <span class="text-sm font-medium text-primary">{{ category }}</span>
                <span class="text-xs text-secondary">
                  {{ events.filter(e => createForm.events.includes(e.value)).length }}/{{ events.length }}
                </span>
              </div>
              <div class="px-3 py-2 flex flex-wrap gap-2">
                <label
                  v-for="event in events"
                  :key="event.value"
                  class="inline-flex items-center gap-1.5 px-2 py-1 rounded-md cursor-pointer transition-colors"
                  :class="createForm.events.includes(event.value) ? 'bg-accent/10 text-accent' : 'bg-surface-alt text-secondary hover:bg-surface-hover'"
                >
                  <input
                    type="checkbox"
                    :checked="createForm.events.includes(event.value)"
                    @change="toggleEvent(event.value, createForm.events)"
                    class="sr-only"
                  />
                  <span class="text-xs">{{ event.label }}</span>
                </label>
              </div>
            </div>
          </div>
          <p class="text-xs text-tertiary mt-1">Select which events trigger this webhook</p>
        </div>

        <!-- Custom Headers -->
        <div>
          <div class="flex items-center justify-between mb-2">
            <label class="text-sm font-medium text-primary">Custom Headers</label>
            <button
              type="button"
              @click="addHeader(true)"
              class="text-xs text-accent hover:text-accent-hover"
            >
              + Add header
            </button>
          </div>
          <div v-if="customHeaders.length > 0" class="flex flex-col gap-2">
            <div v-for="(header, index) in customHeaders" :key="index" class="flex items-center gap-2">
              <input
                v-model="header.key"
                type="text"
                placeholder="Header name"
                class="flex-1 px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent text-sm"
              />
              <input
                v-model="header.value"
                type="text"
                placeholder="Value"
                class="flex-1 px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent text-sm"
              />
              <button
                type="button"
                @click="removeHeader(index, true)"
                class="p-2 text-secondary hover:text-status-error"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            </div>
          </div>
          <p v-else class="text-xs text-tertiary">No custom headers</p>
        </div>

        <!-- Actions -->
        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="showCreateModal = false"
            class="px-4 py-2 text-sm text-secondary hover:text-primary transition-colors"
          >
            Cancel
          </button>
          <button
            type="submit"
            :disabled="isSaving"
            class="px-4 py-2 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors disabled:opacity-50"
          >
            {{ isSaving ? 'Creating...' : 'Create Webhook' }}
          </button>
        </div>
      </form>
    </Modal>

    <!-- Secret Created Modal -->
    <Modal
      :show="showSecretCreated"
      title="Webhook Created"
      size="sm"
      @close="showSecretCreated = false"
    >
      <div class="flex flex-col gap-4">
        <div class="flex items-center gap-2 p-3 bg-status-warning/10 border border-status-warning/20 rounded-lg">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-status-warning flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
          </svg>
          <p class="text-sm text-status-warning">Copy this secret now - it won't be shown again!</p>
        </div>

        <div class="relative">
          <code class="block w-full p-3 bg-surface-alt border border-default rounded-lg text-primary font-mono text-sm break-all">
            {{ createdWebhook?.secret }}
          </code>
          <button
            @click="copySecret(createdWebhook?.secret || '')"
            class="absolute top-2 right-2 p-1.5 text-secondary hover:text-primary hover:bg-surface-hover rounded transition-colors"
            :title="copiedSecret ? 'Copied!' : 'Copy to clipboard'"
          >
            <svg v-if="!copiedSecret" xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
            </svg>
            <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 text-status-success" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
            </svg>
          </button>
        </div>

        <p class="text-xs text-tertiary">
          Use this secret to verify webhook signatures via the <code class="px-1 py-0.5 bg-surface-alt rounded">X-Nosdesk-Signature</code> header
        </p>

        <div class="flex justify-end pt-2">
          <button
            @click="showSecretCreated = false"
            class="px-4 py-2 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors"
          >
            Done
          </button>
        </div>
      </div>
    </Modal>

    <!-- Edit Webhook Modal -->
    <Modal
      :show="showEditModal"
      title="Edit Webhook"
      size="lg"
      @close="showEditModal = false"
    >
      <form @submit.prevent="updateWebhook" class="flex flex-col gap-4">
        <!-- Enabled toggle -->
        <div class="flex items-center justify-between p-3 bg-surface-alt rounded-lg">
          <div>
            <div class="text-sm font-medium text-primary">Enabled</div>
            <div class="text-xs text-secondary">Webhook will receive events when enabled</div>
          </div>
          <button
            type="button"
            @click="editForm.enabled = !editForm.enabled"
            class="relative inline-flex h-6 w-11 items-center rounded-full transition-colors"
            :class="editForm.enabled ? 'bg-accent' : 'bg-surface-hover'"
          >
            <span
              class="inline-block h-4 w-4 transform rounded-full bg-white transition-transform"
              :class="editForm.enabled ? 'translate-x-6' : 'translate-x-1'"
            />
          </button>
        </div>

        <!-- Name -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">Name</label>
          <input
            v-model="editForm.name"
            type="text"
            placeholder="e.g., Slack Notifications"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            required
          />
        </div>

        <!-- URL -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">Payload URL</label>
          <input
            v-model="editForm.url"
            type="url"
            placeholder="https://example.com/webhook"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent font-mono text-sm"
            required
          />
        </div>

        <!-- Secret -->
        <div class="p-3 bg-surface-alt rounded-lg">
          <div class="flex items-center justify-between">
            <div>
              <div class="text-sm font-medium text-primary">Secret</div>
              <div class="text-xs text-secondary font-mono">{{ webhookToEdit?.secret_preview }}</div>
            </div>
            <button
              type="button"
              @click="confirmRegenerateSecret"
              class="text-xs text-status-warning hover:text-status-warning/80"
            >
              Regenerate
            </button>
          </div>
        </div>

        <!-- Events -->
        <div>
          <label class="block text-sm font-medium text-primary mb-2">Events</label>
          <div class="border border-default rounded-lg max-h-64 overflow-y-auto">
            <div v-for="(events, category) in WEBHOOK_EVENT_CATEGORIES" :key="category" class="border-b border-default last:border-b-0">
              <div
                class="px-3 py-2 bg-surface-alt flex items-center justify-between cursor-pointer hover:bg-surface-hover"
                @click="toggleCategory(category, editForm.events || [], false)"
              >
                <span class="text-sm font-medium text-primary">{{ category }}</span>
                <span class="text-xs text-secondary">
                  {{ events.filter(e => (editForm.events || []).includes(e.value)).length }}/{{ events.length }}
                </span>
              </div>
              <div class="px-3 py-2 flex flex-wrap gap-2">
                <label
                  v-for="event in events"
                  :key="event.value"
                  class="inline-flex items-center gap-1.5 px-2 py-1 rounded-md cursor-pointer transition-colors"
                  :class="(editForm.events || []).includes(event.value) ? 'bg-accent/10 text-accent' : 'bg-surface-alt text-secondary hover:bg-surface-hover'"
                >
                  <input
                    type="checkbox"
                    :checked="(editForm.events || []).includes(event.value)"
                    @change="toggleEvent(event.value, editForm.events || [])"
                    class="sr-only"
                  />
                  <span class="text-xs">{{ event.label }}</span>
                </label>
              </div>
            </div>
          </div>
        </div>

        <!-- Custom Headers -->
        <div>
          <div class="flex items-center justify-between mb-2">
            <label class="text-sm font-medium text-primary">Custom Headers</label>
            <button
              type="button"
              @click="addHeader(false)"
              class="text-xs text-accent hover:text-accent-hover"
            >
              + Add header
            </button>
          </div>
          <div v-if="editCustomHeaders.length > 0" class="flex flex-col gap-2">
            <div v-for="(header, index) in editCustomHeaders" :key="index" class="flex items-center gap-2">
              <input
                v-model="header.key"
                type="text"
                placeholder="Header name"
                class="flex-1 px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent text-sm"
              />
              <input
                v-model="header.value"
                type="text"
                placeholder="Value"
                class="flex-1 px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent text-sm"
              />
              <button
                type="button"
                @click="removeHeader(index, false)"
                class="p-2 text-secondary hover:text-status-error"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            </div>
          </div>
          <p v-else class="text-xs text-tertiary">No custom headers</p>
        </div>

        <!-- Actions -->
        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="showEditModal = false"
            class="px-4 py-2 text-sm text-secondary hover:text-primary transition-colors"
          >
            Cancel
          </button>
          <button
            type="submit"
            :disabled="isSaving"
            class="px-4 py-2 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors disabled:opacity-50"
          >
            {{ isSaving ? 'Saving...' : 'Save Changes' }}
          </button>
        </div>
      </form>
    </Modal>

    <!-- Regenerate Secret Confirmation -->
    <Modal
      :show="showRegenerateConfirm"
      title="Regenerate Secret"
      size="sm"
      @close="showRegenerateConfirm = false"
    >
      <div class="flex flex-col gap-4">
        <p class="text-secondary">
          Are you sure you want to regenerate the secret for <strong class="text-primary">{{ webhookToEdit?.name }}</strong>?
        </p>
        <p class="text-sm text-status-warning">
          The current secret will be invalidated. You'll need to update your integration with the new secret.
        </p>

        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="showRegenerateConfirm = false"
            class="px-4 py-2 text-sm text-secondary hover:text-primary transition-colors"
          >
            Cancel
          </button>
          <button
            @click="regenerateSecret"
            :disabled="isSaving"
            class="px-4 py-2 bg-status-warning text-white rounded-lg text-sm hover:bg-status-warning/90 font-medium transition-colors disabled:opacity-50"
          >
            {{ isSaving ? 'Regenerating...' : 'Regenerate' }}
          </button>
        </div>
      </div>
    </Modal>

    <!-- Delete Confirmation Modal -->
    <Modal
      :show="showDeleteConfirm"
      title="Delete Webhook"
      size="sm"
      @close="showDeleteConfirm = false"
    >
      <div class="flex flex-col gap-4">
        <p class="text-secondary">
          Are you sure you want to delete the webhook <strong class="text-primary">{{ webhookToDelete?.name }}</strong>?
        </p>
        <p class="text-sm text-status-error">
          This action cannot be undone. All delivery history will be lost.
        </p>

        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="showDeleteConfirm = false"
            class="px-4 py-2 text-sm text-secondary hover:text-primary transition-colors"
          >
            Cancel
          </button>
          <button
            @click="deleteWebhook"
            :disabled="isSaving"
            class="px-4 py-2 bg-status-error text-white rounded-lg text-sm hover:bg-status-error/90 font-medium transition-colors disabled:opacity-50"
          >
            {{ isSaving ? 'Deleting...' : 'Delete Webhook' }}
          </button>
        </div>
      </div>
    </Modal>

    <!-- Deliveries Modal -->
    <Modal
      :show="showDeliveries"
      :title="`Delivery History - ${webhookForDeliveries?.name}`"
      size="lg"
      @close="showDeliveries = false"
    >
      <div class="flex flex-col gap-4">
        <LoadingSpinner v-if="isLoadingDeliveries" text="Loading deliveries..." />

        <div v-else-if="deliveries.length === 0" class="text-center py-8">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-12 w-12 mx-auto text-tertiary mb-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M20 13V6a2 2 0 00-2-2H6a2 2 0 00-2 2v7m16 0v5a2 2 0 01-2 2H6a2 2 0 01-2-2v-5m16 0h-2.586a1 1 0 00-.707.293l-2.414 2.414a1 1 0 01-.707.293h-3.172a1 1 0 01-.707-.293l-2.414-2.414A1 1 0 006.586 13H4" />
          </svg>
          <p class="text-secondary">No deliveries yet</p>
          <p class="text-xs text-tertiary mt-1">Deliveries will appear here once events are triggered</p>
        </div>

        <div v-else class="max-h-96 overflow-y-auto">
          <div
            v-for="delivery in deliveries"
            :key="delivery.uuid"
            class="flex items-start gap-3 p-3 border-b border-default last:border-b-0"
          >
            <!-- Status badge -->
            <span
              class="px-2 py-0.5 text-xs rounded font-medium flex-shrink-0"
              :class="getDeliveryStatusColor(delivery)"
            >
              {{ delivery.response_status || (delivery.error_message ? 'Error' : 'Pending') }}
            </span>

            <!-- Delivery info -->
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2 text-sm">
                <span class="font-medium text-primary">{{ delivery.event_type }}</span>
                <span v-if="delivery.attempt_number > 1" class="text-xs text-status-warning">
                  Attempt {{ delivery.attempt_number }}
                </span>
              </div>
              <div class="text-xs text-secondary mt-0.5">
                {{ delivery.delivered_at ? formatDate(delivery.delivered_at) : formatDate(delivery.created_at) }}
                <span v-if="delivery.duration_ms"> - {{ delivery.duration_ms }}ms</span>
              </div>
              <div v-if="delivery.error_message" class="text-xs text-status-error mt-1 truncate">
                {{ delivery.error_message }}
              </div>
            </div>
          </div>
        </div>

        <div class="flex justify-end pt-2">
          <button
            @click="showDeliveries = false"
            class="px-4 py-2 bg-surface-alt text-primary rounded-lg text-sm hover:bg-surface-hover font-medium transition-colors"
          >
            Close
          </button>
        </div>
      </div>
    </Modal>
  </div>
</template>
