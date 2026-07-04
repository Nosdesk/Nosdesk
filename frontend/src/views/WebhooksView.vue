<script setup lang="ts">
import { ref, computed } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';

import AlertMessage from '@/components/common/AlertMessage.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Icon from '@/components/common/Icon.vue';
import Modal from '@/components/Modal.vue';
import webhookService from '@nosdesk/core/services/webhookService';
import { formatDistanceToNow } from 'date-fns';
import type {
  Webhook,
  WebhookCreated,
  CreateWebhookRequest,
  UpdateWebhookRequest,
  WebhookDelivery,
} from '@nosdesk/core/types/webhook';
import { WEBHOOK_EVENT_CATEGORIES } from '@nosdesk/core/types/webhook';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

// The webhook list is cached by Pinia Colada keyed here, so navigating
// away and back renders it instantly from cache and revalidates in the
// background. A skeleton shows only on the genuine first load (empty
// cache); see `isFirstLoad`.
const WEBHOOKS_KEY = ['webhooks'] as const;
const queryCache = useQueryCache();
const webhooksQuery = useQuery({
  key: WEBHOOKS_KEY,
  query: () => webhookService.listWebhooks(),
});
const webhooks = computed<Webhook[]>(() =>
  Array.isArray(webhooksQuery.data.value) ? webhooksQuery.data.value : (webhooksQuery.data.value ?? []),
);
const isFirstLoad = computed(
  () => webhooksQuery.status.value === 'pending' && webhooksQuery.data.value === undefined,
);
const loadError = computed(() =>
  webhooksQuery.error.value ? t('admin-webhooks-error-load') : '',
);

// Mutation feedback stays in local refs.
const isSaving = ref(false);
const errorMessage = ref('');
const successMessage = ref('');

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

// Custom headers for create / edit forms. Each row carries a
// stable monotonic `_uid` so the v-for key survives row deletion.
// Index keys would let stale form state bleed into the row that
// takes the deleted row's slot.
interface HeaderRow { _uid: number; key: string; value: string }
let nextHeaderUid = 1
function newHeaderRow(key = '', value = ''): HeaderRow {
  return { _uid: nextHeaderUid++, key, value }
}

const customHeaders = ref<HeaderRow[]>([]);
const editCustomHeaders = ref<HeaderRow[]>([]);

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
  if (!dateStr) return t('admin-webhooks-meta-never');
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

// Convert headers object to array format with stable row uids.
const objectToHeaders = (obj: Record<string, string> | null): HeaderRow[] => {
  return obj ? Object.entries(obj).map(([key, value]) => newHeaderRow(key, value)) : [];
};

// Category label translation (categories are object keys in WEBHOOK_EVENT_CATEGORIES)
const CATEGORY_KEYS: Record<string, string> = {
  Tickets: 'admin-webhooks-category-tickets',
  Comments: 'admin-webhooks-category-comments',
  Attachments: 'admin-webhooks-category-attachments',
  Devices: 'admin-webhooks-category-devices',
  Projects: 'admin-webhooks-category-projects',
  Documentation: 'admin-webhooks-category-documentation',
  Users: 'admin-webhooks-category-users',
};
const categoryLabel = (category: string) => {
  const key = CATEGORY_KEYS[category];
  return key ? t(key) : category;
};

// Event-value label translation (values come from WEBHOOK_EVENTS)
const eventLabel = (value: string) => {
  const key = `admin-webhooks-event-${value.replace('.', '-')}`;
  return t(key);
};

// Get webhook status
const getWebhookStatus = (webhook: Webhook) => {
  if (!webhook.enabled) {
    return { label: t('admin-webhooks-status-disabled'), color: 'text-secondary', bg: 'bg-surface-alt' };
  }
  if (webhook.failure_count >= 5) {
    return { label: t('admin-webhooks-status-failing'), color: 'text-status-error', bg: 'bg-status-error/10' };
  }
  if (webhook.failure_count > 0) {
    return { label: t('admin-webhooks-status-warning'), color: 'text-status-warning', bg: 'bg-status-warning/10' };
  }
  return { label: t('admin-webhooks-status-active'), color: 'text-status-success', bg: 'bg-status-success/10' };
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
    customHeaders.value.push(newHeaderRow());
  } else {
    editCustomHeaders.value.push(newHeaderRow());
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
    errorMessage.value = t('admin-webhooks-error-name-required');
    return;
  }
  if (!createForm.value.url.trim()) {
    errorMessage.value = t('admin-webhooks-error-url-required');
    return;
  }
  if (createForm.value.events.length === 0) {
    errorMessage.value = t('admin-webhooks-error-event-required');
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
    await queryCache.invalidateQueries({ key: WEBHOOKS_KEY });
  } catch (error) {
    errorMessage.value = getErrorMessage(error, t('admin-webhooks-error-create'));
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
    errorMessage.value = t('admin-webhooks-error-name-required');
    return;
  }
  if (!editForm.value.url?.trim()) {
    errorMessage.value = t('admin-webhooks-error-url-required');
    return;
  }
  if (!editForm.value.events || editForm.value.events.length === 0) {
    errorMessage.value = t('admin-webhooks-error-event-required');
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
    successMessage.value = t('admin-webhooks-success-update');
    showEditModal.value = false;
    webhookToEdit.value = null;
    await queryCache.invalidateQueries({ key: WEBHOOKS_KEY });

    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    errorMessage.value = getErrorMessage(error, t('admin-webhooks-error-update'));
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
    successMessage.value = t('admin-webhooks-success-delete');
    showDeleteConfirm.value = false;
    webhookToDelete.value = null;
    await queryCache.invalidateQueries({ key: WEBHOOKS_KEY });

    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    errorMessage.value = getErrorMessage(error, t('admin-webhooks-error-delete'));
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
    successMessage.value = t('admin-webhooks-success-test');
    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    errorMessage.value = getErrorMessage(error, t('admin-webhooks-error-test'));
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
    await webhookService.updateWebhook(webhookToEdit.value.uuid, {
      regenerate_secret: true,
    });
    // The updated webhook will have the new secret preview.
    // We need to show the user the full secret from a special response.
    // For now, show success and close the confirm modal.
    newSecret.value = null; // Backend doesn't return the new secret on regenerate via update
    successMessage.value = t('admin-webhooks-success-regenerate');
    showRegenerateConfirm.value = false;
    await queryCache.invalidateQueries({ key: WEBHOOKS_KEY });
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
    errorMessage.value = getErrorMessage(error, t('admin-webhooks-error-regenerate'));
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

// Display label for a delivery status badge
const deliveryStatusLabel = (delivery: WebhookDelivery): string => {
  if (delivery.response_status) return String(delivery.response_status);
  if (delivery.error_message) return t('admin-webhooks-deliveries-status-error');
  return t('admin-webhooks-deliveries-status-pending');
};
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-2 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
        <div>
          <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('admin-webhooks-title') }}</h1>
          <p class="text-secondary text-sm sm:text-base mt-1">{{ $t('admin-webhooks-subtitle') }}</p>
        </div>
        <button
          @click="openCreateModal"
          class="px-3 py-1.5 bg-accent text-on-accent rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors flex items-center gap-1.5 self-start sm:self-auto"
        >
          <Icon name="add" />
          <span class="hidden xs:inline">{{ $t('admin-webhooks-create') }}</span>
          <span class="xs:hidden">{{ $t('admin-webhooks-create-short') }}</span>
        </button>
      </div>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Error message -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Load error (initial fetch failed with no cached data) -->
      <AlertMessage v-if="loadError && webhooks.length === 0" type="error" :message="loadError" />

      <!-- First-load skeleton: mirrors the webhook-row layout so the
           shell doesn't shift when data arrives. Only shown on a cold
           cache; remounts render cached rows instantly and revalidate
           silently in the background. -->
      <Skeleton
        v-if="isFirstLoad"
        :label="$t('admin-webhooks-loading')"
        class="flex flex-col gap-2 sm:gap-3"
      >
        <div
          v-for="n in 3"
          :key="n"
          class="bg-surface border border-default rounded-lg sm:rounded-xl p-3 sm:p-4 flex items-start gap-3 sm:gap-4"
        >
          <SkeletonBar class="w-8 h-8 sm:w-10 sm:h-10 rounded-lg shrink-0" />
          <div class="flex-1 flex flex-col gap-2">
            <SkeletonBar class="h-4 w-40 max-w-full" />
            <SkeletonBar class="h-3 w-3/4" />
          </div>
        </div>
      </Skeleton>

      <!-- Webhooks list -->
      <div v-else class="flex flex-col gap-4">
        <!-- Active webhooks -->
        <div v-if="enabledWebhooks.length > 0" class="flex flex-col gap-2 sm:gap-3">
          <h2 class="text-sm font-medium text-secondary uppercase tracking-wide">{{ $t('admin-webhooks-section-active') }}</h2>
          <div
            v-for="webhook in enabledWebhooks"
            :key="webhook.uuid"
            class="bg-surface border border-default rounded-lg sm:rounded-xl"
          >
            <div class="p-3 sm:p-4 flex items-start gap-3 sm:gap-4">
              <!-- Webhook icon -->
              <div class="w-8 h-8 sm:w-10 sm:h-10 rounded-lg flex items-center justify-center flex-shrink-0"
                   :class="getWebhookStatus(webhook).bg">
                <Icon name="link" :class="getWebhookStatus(webhook).color" />
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
                    {{ $t('admin-webhooks-failure-count', { count: webhook.failure_count }) }}
                  </span>
                </div>
                <div class="text-xs text-secondary mt-1 truncate font-mono">{{ webhook.url }}</div>
                <div class="flex flex-wrap items-center gap-2 mt-1 text-xs text-secondary">
                  <span>{{ $t('admin-webhooks-meta-secret') }} <code class="px-1 py-0.5 bg-surface-alt rounded">{{ webhook.secret_preview }}</code></span>
                  <span class="text-tertiary">|</span>
                  <span>{{ $t('admin-webhooks-meta-events', { count: webhook.events.length }) }}</span>
                  <span class="text-tertiary">|</span>
                  <span>{{ $t('admin-webhooks-meta-last-triggered', { when: formatDate(webhook.last_triggered_at) }) }}</span>
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
                  :title="$t('admin-webhooks-action-send-test')"
                  :disabled="isSaving"
                >
                  <Icon name="send" />
                </button>
                <button
                  @click="viewDeliveries(webhook)"
                  class="p-1.5 sm:p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-md sm:rounded-lg transition-colors"
                  :title="$t('admin-webhooks-action-view-deliveries')"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-3 7h3m-3 4h3m-6-4h.01M9 16h.01" />
                  </svg>
                </button>
                <button
                  @click="openEditModal(webhook)"
                  class="p-1.5 sm:p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-md sm:rounded-lg transition-colors"
                  :title="$t('admin-webhooks-action-edit')"
                >
                  <Icon name="rename" />
                </button>
                <button
                  @click="confirmDelete(webhook)"
                  class="p-1.5 sm:p-2 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-md sm:rounded-lg transition-colors"
                  :title="$t('admin-webhooks-action-delete')"
                >
                  <Icon name="trash" />
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- Disabled webhooks -->
        <div v-if="disabledWebhooks.length > 0" class="flex flex-col gap-2 sm:gap-3 mt-4">
          <h2 class="text-sm font-medium text-secondary uppercase tracking-wide">{{ $t('admin-webhooks-section-disabled') }}</h2>
          <div
            v-for="webhook in disabledWebhooks"
            :key="webhook.uuid"
            class="bg-surface border border-default rounded-lg sm:rounded-xl opacity-60"
          >
            <div class="p-3 sm:p-4 flex items-start gap-3 sm:gap-4">
              <!-- Webhook icon -->
              <div class="w-8 h-8 sm:w-10 sm:h-10 rounded-lg bg-surface-alt flex items-center justify-center flex-shrink-0">
                <Icon name="link" class="text-secondary" />
              </div>

              <!-- Webhook info -->
              <div class="flex-1 min-w-0">
                <div class="flex flex-col sm:flex-row sm:items-center gap-1 sm:gap-2">
                  <h3 class="font-medium text-secondary text-sm sm:text-base truncate">{{ webhook.name }}</h3>
                  <span class="px-1.5 py-0.5 text-xs bg-surface-alt text-secondary rounded">{{ $t('admin-webhooks-status-disabled') }}</span>
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
                  :title="$t('admin-webhooks-action-edit')"
                >
                  <Icon name="rename" />
                </button>
                <button
                  @click="confirmDelete(webhook)"
                  class="p-1.5 sm:p-2 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-md sm:rounded-lg transition-colors"
                  :title="$t('admin-webhooks-action-delete')"
                >
                  <Icon name="trash" />
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- Empty state -->
        <EmptyState
          v-if="webhooks.length === 0 && !isFirstLoad"
          icon="link"
          :title="$t('empty-webhooks-title')"
          :description="$t('empty-webhooks-description')"
          :action-label="$t('admin-webhooks-create')"
          variant="card"
          @action="openCreateModal"
        />
      </div>
    </div>

    <!-- Create Webhook Modal -->
    <Modal
      :show="showCreateModal"
      :title="$t('admin-webhooks-modal-create-title')"
      size="lg"
      @close="showCreateModal = false"
    >
      <form @submit.prevent="createWebhook" class="flex flex-col gap-4">
        <!-- Name -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">{{ $t('admin-webhooks-form-name-label') }}</label>
          <input
            v-model="createForm.name"
            type="text"
            :placeholder="$t('admin-webhooks-form-name-placeholder')"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            required
          />
        </div>

        <!-- URL -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">{{ $t('admin-webhooks-form-url-label') }}</label>
          <input
            v-model="createForm.url"
            type="url"
            :placeholder="$t('admin-webhooks-form-url-placeholder')"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent font-mono text-sm"
            required
          />
          <p class="text-xs text-tertiary mt-1">{{ $t('admin-webhooks-form-url-hint') }}</p>
        </div>

        <!-- Events -->
        <div>
          <label class="block text-sm font-medium text-primary mb-2">{{ $t('admin-webhooks-form-events-label') }}</label>
          <div class="border border-default rounded-lg max-h-64 overflow-y-auto">
            <div v-for="(events, category) in WEBHOOK_EVENT_CATEGORIES" :key="category" class="border-b border-default last:border-b-0">
              <div
                class="px-3 py-2 bg-surface-alt flex items-center justify-between cursor-pointer hover:bg-surface-hover"
                @click="toggleCategory(category, createForm.events, true)"
              >
                <span class="text-sm font-medium text-primary">{{ categoryLabel(category) }}</span>
                <span class="text-xs text-secondary">
                  {{ $t('admin-webhooks-form-events-count', {
                    selected: events.filter(e => createForm.events.includes(e.value)).length,
                    total: events.length,
                  }) }}
                </span>
              </div>
              <!-- Toggle-chip selector: each chip is a single
                   <button> with role="checkbox" + aria-checked, so
                   keyboard activation (Space / Enter) and screen-
                   reader semantics come for free without an
                   sr-only hidden input. The visible chip is the
                   affordance; its bg-accent / bg-surface-alt swap
                   communicates the selection state. -->
              <div class="px-3 py-2 flex flex-wrap gap-2">
                <button
                  v-for="event in events"
                  :key="event.value"
                  type="button"
                  role="checkbox"
                  :aria-checked="createForm.events.includes(event.value)"
                  class="inline-flex items-center gap-1.5 px-2 py-1 rounded-md cursor-pointer transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
                  :class="createForm.events.includes(event.value) ? 'bg-accent/10 text-accent' : 'bg-surface-alt text-secondary hover:bg-surface-hover'"
                  @click="toggleEvent(event.value, createForm.events)"
                >
                  <span class="text-xs">{{ eventLabel(event.value) }}</span>
                </button>
              </div>
            </div>
          </div>
          <p class="text-xs text-tertiary mt-1">{{ $t('admin-webhooks-form-events-hint') }}</p>
        </div>

        <!-- Custom Headers -->
        <div>
          <div class="flex items-center justify-between mb-2">
            <label class="text-sm font-medium text-primary">{{ $t('admin-webhooks-form-headers-label') }}</label>
            <button
              type="button"
              @click="addHeader(true)"
              class="text-xs text-accent hover:text-accent-hover"
            >
              {{ $t('admin-webhooks-form-headers-add') }}
            </button>
          </div>
          <div v-if="customHeaders.length > 0" class="flex flex-col gap-2">
            <div v-for="(header, index) in customHeaders" :key="header._uid" class="flex items-center gap-2">
              <input
                v-model="header.key"
                type="text"
                :placeholder="$t('admin-webhooks-form-headers-name-placeholder')"
                class="flex-1 px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent text-sm"
              />
              <input
                v-model="header.value"
                type="text"
                :placeholder="$t('admin-webhooks-form-headers-value-placeholder')"
                class="flex-1 px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent text-sm"
              />
              <button
                type="button"
                @click="removeHeader(index, true)"
                class="p-2 text-secondary hover:text-status-error"
              >
                <Icon name="close" />
              </button>
            </div>
          </div>
          <p v-else class="text-xs text-tertiary">{{ $t('admin-webhooks-form-headers-empty') }}</p>
        </div>

        <!-- Actions -->
        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="showCreateModal = false"
            class="px-4 py-2 text-sm text-secondary hover:text-primary transition-colors"
          >
            {{ $t('admin-webhooks-form-cancel') }}
          </button>
          <button
            type="submit"
            :disabled="isSaving"
            class="px-4 py-2 bg-accent text-on-accent rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors disabled:opacity-50"
          >
            {{ isSaving ? $t('admin-webhooks-form-creating') : $t('admin-webhooks-form-create') }}
          </button>
        </div>
      </form>
    </Modal>

    <!-- Secret Created Modal -->
    <Modal
      :show="showSecretCreated"
      :title="$t('admin-webhooks-modal-secret-title')"
      size="sm"
      @close="showSecretCreated = false"
    >
      <div class="flex flex-col gap-4">
        <div class="flex items-start gap-2 p-3 bg-status-warning/10 border border-status-warning/20 rounded-lg">
          <Icon name="warning" size="md" class="text-status-warning flex-shrink-0 mt-0.5" />
          <p class="text-sm text-status-warning">{{ $t('admin-webhooks-secret-warning') }}</p>
        </div>

        <div class="relative">
          <code class="block w-full p-3 bg-surface-alt border border-default rounded-lg text-primary font-mono text-sm break-all">
            {{ createdWebhook?.secret }}
          </code>
          <button
            @click="copySecret(createdWebhook?.secret || '')"
            class="absolute top-2 right-2 p-1.5 text-secondary hover:text-primary hover:bg-surface-hover rounded transition-colors"
            :title="copiedSecret ? $t('admin-webhooks-secret-copied') : $t('admin-webhooks-secret-copy')"
          >
            <Icon v-if="!copiedSecret" name="copy" />
            <Icon v-else name="check" class="text-status-success" />
          </button>
        </div>

        <p class="text-xs text-tertiary">
          {{ $t('admin-webhooks-secret-helper-before') }} <code class="px-1 py-0.5 bg-surface-alt rounded">X-Nosdesk-Signature</code> {{ $t('admin-webhooks-secret-helper-after') }}
        </p>

        <div class="flex justify-end pt-2">
          <button
            @click="showSecretCreated = false"
            class="px-4 py-2 bg-accent text-on-accent rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors"
          >
            {{ $t('admin-webhooks-secret-done') }}
          </button>
        </div>
      </div>
    </Modal>

    <!-- Edit Webhook Modal -->
    <Modal
      :show="showEditModal"
      :title="$t('admin-webhooks-modal-edit-title')"
      size="lg"
      @close="showEditModal = false"
    >
      <form @submit.prevent="updateWebhook" class="flex flex-col gap-4">
        <!-- Enabled toggle -->
        <div class="flex items-center justify-between p-3 bg-surface-alt rounded-lg">
          <div>
            <div class="text-sm font-medium text-primary">{{ $t('admin-webhooks-form-enabled-label') }}</div>
            <div class="text-xs text-secondary">{{ $t('admin-webhooks-form-enabled-description') }}</div>
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
          <label class="block text-sm font-medium text-primary mb-1">{{ $t('admin-webhooks-form-name-label') }}</label>
          <input
            v-model="editForm.name"
            type="text"
            :placeholder="$t('admin-webhooks-form-name-placeholder')"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            required
          />
        </div>

        <!-- URL -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">{{ $t('admin-webhooks-form-url-label') }}</label>
          <input
            v-model="editForm.url"
            type="url"
            :placeholder="$t('admin-webhooks-form-url-placeholder')"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent font-mono text-sm"
            required
          />
        </div>

        <!-- Secret -->
        <div class="p-3 bg-surface-alt rounded-lg">
          <div class="flex items-center justify-between">
            <div>
              <div class="text-sm font-medium text-primary">{{ $t('admin-webhooks-form-secret-label') }}</div>
              <div class="text-xs text-secondary font-mono">{{ webhookToEdit?.secret_preview }}</div>
            </div>
            <button
              type="button"
              @click="confirmRegenerateSecret"
              class="text-xs text-status-warning hover:text-status-warning/80"
            >
              {{ $t('admin-webhooks-form-secret-regenerate') }}
            </button>
          </div>
        </div>

        <!-- Events -->
        <div>
          <label class="block text-sm font-medium text-primary mb-2">{{ $t('admin-webhooks-form-events-label') }}</label>
          <div class="border border-default rounded-lg max-h-64 overflow-y-auto">
            <div v-for="(events, category) in WEBHOOK_EVENT_CATEGORIES" :key="category" class="border-b border-default last:border-b-0">
              <div
                class="px-3 py-2 bg-surface-alt flex items-center justify-between cursor-pointer hover:bg-surface-hover"
                @click="toggleCategory(category, editForm.events || [], false)"
              >
                <span class="text-sm font-medium text-primary">{{ categoryLabel(category) }}</span>
                <span class="text-xs text-secondary">
                  {{ $t('admin-webhooks-form-events-count', {
                    selected: events.filter(e => (editForm.events || []).includes(e.value)).length,
                    total: events.length,
                  }) }}
                </span>
              </div>
              <div class="px-3 py-2 flex flex-wrap gap-2">
                <button
                  v-for="event in events"
                  :key="event.value"
                  type="button"
                  role="checkbox"
                  :aria-checked="(editForm.events || []).includes(event.value)"
                  class="inline-flex items-center gap-1.5 px-2 py-1 rounded-md cursor-pointer transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface"
                  :class="(editForm.events || []).includes(event.value) ? 'bg-accent/10 text-accent' : 'bg-surface-alt text-secondary hover:bg-surface-hover'"
                  @click="toggleEvent(event.value, editForm.events || [])"
                >
                  <span class="text-xs">{{ eventLabel(event.value) }}</span>
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- Custom Headers -->
        <div>
          <div class="flex items-center justify-between mb-2">
            <label class="text-sm font-medium text-primary">{{ $t('admin-webhooks-form-headers-label') }}</label>
            <button
              type="button"
              @click="addHeader(false)"
              class="text-xs text-accent hover:text-accent-hover"
            >
              {{ $t('admin-webhooks-form-headers-add') }}
            </button>
          </div>
          <div v-if="editCustomHeaders.length > 0" class="flex flex-col gap-2">
            <div v-for="(header, index) in editCustomHeaders" :key="header._uid" class="flex items-center gap-2">
              <input
                v-model="header.key"
                type="text"
                :placeholder="$t('admin-webhooks-form-headers-name-placeholder')"
                class="flex-1 px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent text-sm"
              />
              <input
                v-model="header.value"
                type="text"
                :placeholder="$t('admin-webhooks-form-headers-value-placeholder')"
                class="flex-1 px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent text-sm"
              />
              <button
                type="button"
                @click="removeHeader(index, false)"
                class="p-2 text-secondary hover:text-status-error"
              >
                <Icon name="close" />
              </button>
            </div>
          </div>
          <p v-else class="text-xs text-tertiary">{{ $t('admin-webhooks-form-headers-empty') }}</p>
        </div>

        <!-- Actions -->
        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="showEditModal = false"
            class="px-4 py-2 text-sm text-secondary hover:text-primary transition-colors"
          >
            {{ $t('admin-webhooks-form-cancel') }}
          </button>
          <button
            type="submit"
            :disabled="isSaving"
            class="px-4 py-2 bg-accent text-on-accent rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors disabled:opacity-50"
          >
            {{ isSaving ? $t('admin-webhooks-form-saving') : $t('admin-webhooks-form-save') }}
          </button>
        </div>
      </form>
    </Modal>

    <!-- Regenerate Secret Confirmation -->
    <Modal
      :show="showRegenerateConfirm"
      :title="$t('admin-webhooks-modal-regenerate-title')"
      size="sm"
      @close="showRegenerateConfirm = false"
    >
      <div class="flex flex-col gap-4">
        <p class="text-secondary">
          {{ $t('admin-webhooks-regenerate-question', { name: webhookToEdit?.name || '' }) }}
        </p>
        <p class="text-sm text-status-warning">
          {{ $t('admin-webhooks-regenerate-warning') }}
        </p>

        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="showRegenerateConfirm = false"
            class="px-4 py-2 text-sm text-secondary hover:text-primary transition-colors"
          >
            {{ $t('admin-webhooks-form-cancel') }}
          </button>
          <button
            @click="regenerateSecret"
            :disabled="isSaving"
            class="px-4 py-2 bg-status-warning text-white rounded-lg text-sm hover:bg-status-warning/90 font-medium transition-colors disabled:opacity-50"
          >
            {{ isSaving ? $t('admin-webhooks-regenerate-running') : $t('admin-webhooks-regenerate-confirm') }}
          </button>
        </div>
      </div>
    </Modal>

    <!-- Delete Confirmation Modal -->
    <Modal
      :show="showDeleteConfirm"
      :title="$t('admin-webhooks-modal-delete-title')"
      size="sm"
      @close="showDeleteConfirm = false"
    >
      <div class="flex flex-col gap-4">
        <p class="text-secondary">
          {{ $t('admin-webhooks-delete-question', { name: webhookToDelete?.name || '' }) }}
        </p>
        <p class="text-sm text-status-error">
          {{ $t('admin-webhooks-delete-warning') }}
        </p>

        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="showDeleteConfirm = false"
            class="px-4 py-2 text-sm text-secondary hover:text-primary transition-colors"
          >
            {{ $t('admin-webhooks-form-cancel') }}
          </button>
          <button
            @click="deleteWebhook"
            :disabled="isSaving"
            class="px-4 py-2 bg-status-error text-white rounded-lg text-sm hover:bg-status-error/90 font-medium transition-colors disabled:opacity-50"
          >
            {{ isSaving ? $t('admin-webhooks-delete-running') : $t('admin-webhooks-delete-confirm') }}
          </button>
        </div>
      </div>
    </Modal>

    <!-- Deliveries Modal -->
    <Modal
      :show="showDeliveries"
      :title="$t('admin-webhooks-modal-deliveries-title', { name: webhookForDeliveries?.name || '' })"
      size="lg"
      @close="showDeliveries = false"
    >
      <div class="flex flex-col gap-4">
        <LoadingSpinner v-if="isLoadingDeliveries" :text="$t('admin-webhooks-deliveries-loading')" />

        <div v-else-if="deliveries.length === 0" class="text-center py-8">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-12 w-12 mx-auto text-tertiary mb-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M20 13V6a2 2 0 00-2-2H6a2 2 0 00-2 2v7m16 0v5a2 2 0 01-2 2H6a2 2 0 01-2-2v-5m16 0h-2.586a1 1 0 00-.707.293l-2.414 2.414a1 1 0 01-.707.293h-3.172a1 1 0 01-.707-.293l-2.414-2.414A1 1 0 006.586 13H4" />
          </svg>
          <p class="text-secondary">{{ $t('admin-webhooks-deliveries-empty-title') }}</p>
          <p class="text-xs text-tertiary mt-1">{{ $t('admin-webhooks-deliveries-empty-description') }}</p>
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
              {{ deliveryStatusLabel(delivery) }}
            </span>

            <!-- Delivery info -->
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2 text-sm">
                <span class="font-medium text-primary">{{ delivery.event_type }}</span>
                <span v-if="delivery.attempt_number > 1" class="text-xs text-status-warning">
                  {{ $t('admin-webhooks-deliveries-attempt', { number: delivery.attempt_number }) }}
                </span>
              </div>
              <div class="text-xs text-secondary mt-0.5">
                {{ delivery.delivered_at ? formatDate(delivery.delivered_at) : formatDate(delivery.created_at) }}
                <span v-if="delivery.duration_ms"> - {{ $t('admin-webhooks-deliveries-duration', { ms: delivery.duration_ms }) }}</span>
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
            {{ $t('admin-webhooks-deliveries-close') }}
          </button>
        </div>
      </div>
    </Modal>
  </div>
</template>
