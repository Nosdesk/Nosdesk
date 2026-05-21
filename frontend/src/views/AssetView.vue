<script setup lang="ts">
import { ref, computed, watch, onMounted, nextTick } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { formatDateTime } from '@/utils/dateUtils';
import BackButton from '@/components/common/BackButton.vue';
import DeleteButton from '@/components/common/DeleteButton.vue';
import InlineEdit from '@/components/common/InlineEdit.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';
import UserCard from '@/components/UserCard.vue';
import UserSelectionModal from '@/components/UserSelectionModal.vue';
import DeviceGroups from '@/components/AssetGroups.vue';
import AssetUsageHistory from '@/components/assets/AssetUsageHistory.vue';
import PluginSlot from '@/plugins/components/PluginSlot.vue';
import Modal from '@/components/Modal.vue';
import { getDeviceById, updateDevice, createDevice, deleteDevice, unmanageDevice } from '@/services/assetService';
import { assetKindsService, type AssetKind } from '@/services/assetKindsService';
import { useSSEListeners } from '@/composables/useSSEListeners';
import type { DeviceUpdatedEventData, DeviceDeletedEventData } from '@/types/sse';
import type { Asset, AssetFormData } from '@/types/asset';
import DynamicAttributeForm from '@/components/assets/DynamicAttributeForm.vue';

const route = useRoute();
const router = useRouter();
const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const emit = defineEmits(['update:device']);

// State
const device = ref<Asset | null>(null);
const loading = ref(true);
const error = ref<string | null>(null);
const isSaving = ref(false);
const showUserSelectionModal = ref(false);
const showUnmanageModal = ref(false);
const unmanageError = ref<string | null>(null);
const hostnameRef = ref<HTMLInputElement | null>(null);
const selectedUser = ref<{ uuid: string; name: string; email: string; role: string } | null>(null);

const editValues = ref({
  name: '',
  manufacturer: '',
  model: '',
  serial_number: '',
  location: '',
  purchase_date: '' as string,
  asset_tag: '' as string,
});

// Asset-kind registry state. Loaded eagerly on mount so the
// picker is populated before the create form renders; in edit
// mode it backs the read-only "Kind" row at the bottom of the
// details card.
const kinds = ref<AssetKind[]>([]);
const selectedKindSlug = ref<string>('device');
const attributeDraft = ref<Record<string, unknown>>({});

const selectedKind = computed<AssetKind | null>(
  () => kinds.value.find((k) => k.slug === selectedKindSlug.value) ?? null,
);

const selectedKindSchema = computed(
  () => (selectedKind.value?.attribute_schema as Record<string, unknown>) ?? null,
);

async function loadKinds() {
  try {
    kinds.value = await assetKindsService.list();
  } catch (err) {
    // Non-fatal: fall back to a single hard-coded 'device' option
    // so the form still functions for non-admins or if the
    // endpoint is unavailable.
    console.warn('asset-kinds list failed; defaulting to device only', err);
  }
}

// Computed
const isCreationMode = computed(() => !route.params.id || route.params.id === 'new');

const fromTicket = computed(() =>
  route.query.fromTicket ? Number(route.query.fromTicket) : null
);

const isSynced = computed(() => device.value != null && !device.value.is_editable);

// Data fetching
const fetchDeviceData = async () => {
  try {
    loading.value = true;
    error.value = null;

    if (isCreationMode.value) {
      editValues.value = {
        name: '', manufacturer: '', model: '',
        serial_number: '', location: '',
        purchase_date: '', asset_tag: '',
      };
      emit('update:device', null);
      loading.value = false;
      await nextTick();
      hostnameRef.value?.focus();
      return;
    }

    const deviceId = Number(route.params.id);
    if (isNaN(deviceId)) {
      error.value = t('asset-detail-error-invalid-id');
      loading.value = false;
      return;
    }

    device.value = await getDeviceById(deviceId);
    editValues.value = {
      name: device.value.name,
      manufacturer: device.value.manufacturer || '',
      model: device.value.model,
      serial_number: device.value.serial_number,
      location: device.value.location || '',
      purchase_date: device.value.purchase_date || '',
      asset_tag: device.value.asset_tag || '',
    };
    // Hydrate the kind picker and attribute draft so the kind
    // section + DynamicAttributeForm render the row's actual
    // attributes (which is where hostname / OS / warranty etc.
    // live after Pass B).
    selectedKindSlug.value = device.value.kind ?? 'generic';
    attributeDraft.value = { ...(device.value.attributes ?? {}) };
  } catch (e) {
    error.value = t('asset-detail-error-load');
    console.error('Error loading device:', e);
  } finally {
    loading.value = false;
  }
};

// Field saving (edit mode)
const saveField = async (field: keyof typeof editValues.value) => {
  if (!device.value) return;

  try {
    isSaving.value = true;
    const updatedDevice = await updateDevice(device.value.id, {
      [field]: editValues.value[field]
    });
    device.value = { ...device.value, ...updatedDevice };
  } catch (err) {
    console.error('Error saving device field:', err);
    if (device.value) {
      editValues.value[field] = (device.value[field as keyof Asset] as string) || '';
    }
  } finally {
    isSaving.value = false;
  }
};

// Asset creation
const saveDevice = async () => {
  try {
    isSaving.value = true;
    const deviceData: AssetFormData = {
      // Fall back to the hostname attribute for the row's display
      // name if the admin hasn't typed one — IT-desk muscle memory
      // sets the kind's hostname and lets the form auto-name.
      name: editValues.value.name || (attributeDraft.value.hostname as string | undefined) || '',
      manufacturer: editValues.value.manufacturer,
      model: editValues.value.model,
      serial_number: editValues.value.serial_number,
      location: editValues.value.location || null,
      purchase_date: editValues.value.purchase_date || null,
      asset_tag: editValues.value.asset_tag || null,
      primary_user_uuid: selectedUser.value?.uuid || undefined,
      kind: selectedKindSlug.value,
      attributes: attributeDraft.value,
    };
    const newDevice = await createDevice(deviceData);
    router.replace(`/assets/${newDevice.id}`);
  } catch (err) {
    console.error('Error creating device:', err);
    error.value = t('asset-detail-error-create');
  } finally {
    isSaving.value = false;
  }
};

// User selection
const handleUserSelection = async (user: { uuid: string; name: string; email: string; role: string }) => {
  if (isCreationMode.value) {
    // In create mode, just store the selection locally
    selectedUser.value = user.uuid ? user : null;
    return;
  }

  if (!device.value) return;

  try {
    isSaving.value = true;
    const updatedDevice = await updateDevice(device.value.id, {
      primary_user_uuid: user.uuid || null
    });
    device.value = { ...device.value, ...updatedDevice };
  } catch (err) {
    console.error('Error updating device user:', err);
  } finally {
    isSaving.value = false;
  }
};

// Asset deletion
const handleDeleteDevice = async () => {
  if (!device.value) return;

  try {
    await deleteDevice(device.value.id);
    router.push('/assets');
  } catch (err) {
    console.error('Error deleting device:', err);
    error.value = t('asset-detail-error-delete');
  }
};

// Unmanage device
const handleUnmanageDevice = () => {
  if (!device.value) return;
  unmanageError.value = null;
  showUnmanageModal.value = true;
};

const confirmUnmanageDevice = async () => {
  if (!device.value) return;

  try {
    isSaving.value = true;
    unmanageError.value = null;
    const updatedDevice = await unmanageDevice(device.value.id);
    device.value = updatedDevice;
    showUnmanageModal.value = false;
  } catch (err) {
    console.error('Error unmanaging device:', err);
    unmanageError.value = t('asset-detail-error-unmanage');
  } finally {
    isSaving.value = false;
  }
};

// Watchers
watch(device, (newDevice) => {
  if (newDevice) {
    emit('update:device', newDevice);
  }
}, { immediate: true, deep: true });

watch(() => route.params.id, () => {
  fetchDeviceData();
});

// SSE integration for real-time updates
const { on } = useSSEListeners();

on('asset-updated', (data) => {
  const event = data as DeviceUpdatedEventData;
  if (!device.value || event.device_id !== device.value.id) return;

  const field = event.field as keyof typeof editValues.value;
  if (field in editValues.value) {
    const val = typeof event.value === 'string' ? event.value : String(event.value ?? '');
    editValues.value[field] = val;
  }
  // Also update the device ref for non-editable display fields
  if (event.field in device.value) {
    (device.value as Record<string, unknown>)[event.field] = event.value;
  }
});

on('asset-deleted', (data) => {
  const event = data as DeviceDeletedEventData;
  if (!device.value || event.device_id !== device.value.id) return;
  router.push('/assets');
});

// Lifecycle
onMounted(() => {
  loadKinds();
  fetchDeviceData();
});
</script>

<template>
  <div class="flex-1">
    <!-- Loading -->
    <div v-if="loading" class="flex justify-center items-center min-h-[200px]">
      <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-accent"></div>
    </div>

    <!-- Main content -->
    <div v-else-if="device || isCreationMode" class="flex flex-col">
      <!-- Navigation bar -->
      <div class="pt-4 px-6 flex justify-between items-center">
        <div class="flex items-center gap-4">
          <BackButton
            v-if="fromTicket"
            :fallbackRoute="`/tickets/${fromTicket}`"
            :label="$t('asset-detail-back-to-ticket', { id: fromTicket })"
          />
          <BackButton v-else fallbackRoute="/assets" :label="$t('asset-detail-back-to-devices')" />

          <div v-if="isSynced" class="flex items-center gap-2 text-sm">
            <div class="w-2 h-2 rounded-full bg-accent"></div>
            <span class="text-secondary">{{ $t('asset-detail-readonly') }}</span>
          </div>
        </div>

        <DeleteButton
          v-if="!isCreationMode && device?.is_editable"
          fallbackRoute="/assets"
          :itemName="$t('asset-detail-delete-item-name')"
          @delete="handleDeleteDevice"
        />
      </div>

      <!-- Content area -->
      <div class="flex flex-col gap-6 px-6 py-4 mx-auto w-full max-w-8xl">
        <AlertMessage v-if="error" type="error" :message="error" />

        <!-- Kind picker + dynamic attribute form. In creation
             mode the admin chooses the kind and fills in any
             per-kind attributes; in edit mode we show a
             read-only summary so the kind is visible without
             surfacing an edit affordance we don't yet support. -->
        <SectionCard v-if="kinds.length > 0" content-padding="p-4">
          <template #title>{{ $t('asset-detail-section-kind') }}</template>
          <div class="flex flex-col gap-4">
            <div class="flex flex-col gap-1.5">
              <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">
                {{ $t('asset-detail-field-kind') }}
              </h3>
              <select
                v-if="isCreationMode"
                v-model="selectedKindSlug"
                class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary"
              >
                <option v-for="k in kinds" :key="k.slug" :value="k.slug">
                  {{ k.label }}
                </option>
              </select>
              <p v-else class="text-sm text-primary">
                {{ selectedKind?.label ?? selectedKindSlug }}
              </p>
              <p v-if="selectedKind?.description" class="text-xs text-tertiary">
                {{ selectedKind.description }}
              </p>
            </div>
            <DynamicAttributeForm
              v-if="selectedKindSchema"
              :schema="selectedKindSchema"
              v-model="attributeDraft"
              :disabled="!isCreationMode"
            />
          </div>
        </SectionCard>

        <div class="grid grid-cols-1 md:grid-cols-2 gap-6 items-start">
          <!-- Left column: Asset Details -->
          <SectionCard content-padding="p-4">
            <template #title>{{ $t('asset-detail-section-details') }}</template>

            <div class="flex flex-col gap-4">
              <!-- Name -->
              <div class="flex flex-col gap-1.5">
                <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('asset-detail-field-name') }}</h3>
                <input
                  v-if="isCreationMode"
                  v-model="editValues.name"
                  type="text"
                  :placeholder="$t('asset-detail-field-name-placeholder-create')"
                  class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                />
                <InlineEdit
                  v-else
                  v-model="editValues.name"
                  :placeholder="$t('asset-detail-field-name-placeholder-edit')"
                  text-size="sm"
                  :can-edit="device?.is_editable ?? false"
                  @update:modelValue="() => saveField('name')"
                />
              </div>

              <!-- Hostname / OS / warranty / Microsoft Graph
                   IDs now live as per-kind attributes; the
                   DynamicAttributeForm in the Kind section above
                   renders them through the kind's
                   attribute_schema. -->

              <!-- Serial Number -->
              <div class="flex flex-col gap-1.5">
                <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('asset-detail-field-serial') }}</h3>
                <input
                  v-if="isCreationMode"
                  v-model="editValues.serial_number"
                  type="text"
                  :placeholder="$t('asset-detail-field-serial-placeholder-create')"
                  class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                />
                <InlineEdit
                  v-else
                  v-model="editValues.serial_number"
                  :placeholder="device?.serial_number || $t('asset-detail-field-serial-placeholder-edit')"
                  text-size="sm"
                  :can-edit="device?.is_editable ?? false"
                  @update:modelValue="() => saveField('serial_number')"
                />
              </div>

              <!-- Manufacturer + Model side-by-side -->
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-4 pt-2 border-t border-default">
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('asset-detail-field-manufacturer') }}</h3>
                  <input
                    v-if="isCreationMode"
                    v-model="editValues.manufacturer"
                    type="text"
                    :placeholder="$t('asset-detail-field-manufacturer-placeholder-create')"
                    class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                  />
                  <InlineEdit
                    v-else
                    v-model="editValues.manufacturer"
                    :placeholder="device?.manufacturer || $t('asset-detail-field-manufacturer-placeholder-edit')"
                    text-size="sm"
                    :can-edit="device?.is_editable ?? false"
                    @update:modelValue="() => saveField('manufacturer')"
                  />
                </div>

                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('asset-detail-field-model') }}</h3>
                  <input
                    v-if="isCreationMode"
                    v-model="editValues.model"
                    type="text"
                    :placeholder="$t('asset-detail-field-model-placeholder-create')"
                    class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                  />
                  <InlineEdit
                    v-else
                    v-model="editValues.model"
                    :placeholder="device?.model || $t('asset-detail-field-model-placeholder-edit')"
                    text-size="sm"
                    :can-edit="device?.is_editable ?? false"
                    @update:modelValue="() => saveField('model')"
                  />
                </div>
              </div>

              <!-- Purchase Date + Asset Tag -->
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('asset-detail-field-purchase-date') }}</h3>
                  <input
                    v-if="isCreationMode || device?.is_editable"
                    v-model="editValues.purchase_date"
                    type="date"
                    class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2 text-primary focus:outline-none focus:ring-2 focus:ring-accent/50 text-sm"
                    @change="() => { if (!isCreationMode) saveField('purchase_date') }"
                  />
                  <p v-else class="text-primary text-sm">{{ device?.purchase_date || '-' }}</p>
                </div>
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('asset-detail-field-asset-tag') }}</h3>
                  <input
                    v-if="isCreationMode"
                    v-model="editValues.asset_tag"
                    type="text"
                    :placeholder="$t('asset-detail-field-asset-tag-placeholder-create')"
                    class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50 text-sm"
                  />
                  <InlineEdit
                    v-else
                    v-model="editValues.asset_tag"
                    :placeholder="$t('asset-detail-field-asset-tag-placeholder-edit')"
                    text-size="sm"
                    :can-edit="device?.is_editable ?? false"
                    @update:modelValue="() => saveField('asset_tag')"
                  />
                </div>
              </div>
            </div>
          </SectionCard>

          <!-- Right column -->
          <div v-if="isCreationMode || device" class="flex flex-col gap-6">
            <!-- Primary User (create mode) -->
            <SectionCard v-if="isCreationMode" content-padding="p-4">
              <template #title>{{ $t('asset-detail-section-primary-user') }}</template>

              <div v-if="selectedUser" class="flex flex-col gap-4">
                <UserCard :user="selectedUser" avatar-size="lg" />

                <button
                  @click="showUserSelectionModal = true"
                  class="w-full px-4 py-2.5 bg-accent text-white rounded-lg hover:bg-accent/80 transition-colors text-sm font-medium flex items-center justify-center gap-2"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 7h12m0 0l-4-4m4 4l-4 4m0 6H4m0 0l4 4m-4-4l4-4" />
                  </svg>
                  {{ $t('asset-detail-action-change-user') }}
                </button>
              </div>

              <div v-else class="flex flex-col items-center py-8 gap-4">
                <div class="inline-flex items-center justify-center w-12 h-12 bg-surface-alt rounded-full">
                  <Icon name="user" size="md" class="text-secondary" />
                </div>
                <p class="text-secondary text-sm">{{ $t('asset-detail-no-user-assigned') }}</p>

                <button
                  @click="showUserSelectionModal = true"
                  class="px-4 py-2.5 bg-accent text-white rounded-lg hover:bg-accent/80 transition-colors text-sm font-medium flex items-center gap-2"
                >
                  <Icon name="add" />
                  {{ $t('asset-detail-action-assign-user') }}
                </button>
              </div>
            </SectionCard>

            <!-- Primary User (edit mode) -->
            <SectionCard v-if="!isCreationMode && device" content-padding="p-4">
              <template #title>{{ $t('asset-detail-section-primary-user') }}</template>

              <div v-if="device.primary_user" class="flex flex-col gap-4">
                <UserCard :user="device.primary_user" avatar-size="lg" />

                <button
                  v-if="device.is_editable"
                  @click="showUserSelectionModal = true"
                  class="w-full px-4 py-2.5 bg-accent text-white rounded-lg hover:bg-accent/80 transition-colors text-sm font-medium flex items-center justify-center gap-2"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 7h12m0 0l-4-4m4 4l-4 4m0 6H4m0 0l4 4m-4-4l4-4" />
                  </svg>
                  {{ $t('asset-detail-action-change-user') }}
                </button>
              </div>

              <div v-else class="flex flex-col items-center py-8 gap-4">
                <div class="inline-flex items-center justify-center w-12 h-12 bg-surface-alt rounded-full">
                  <Icon name="user" size="md" class="text-secondary" />
                </div>
                <p class="text-secondary text-sm">{{ $t('asset-detail-no-user-assigned') }}</p>

                <button
                  v-if="device.is_editable"
                  @click="showUserSelectionModal = true"
                  class="px-4 py-2.5 bg-accent text-white rounded-lg hover:bg-accent/80 transition-colors text-sm font-medium flex items-center gap-2"
                >
                  <Icon name="add" />
                  {{ $t('asset-detail-action-assign-user') }}
                </button>
              </div>
            </SectionCard>

            <!-- Groups (edit mode only) -->
            <DeviceGroups v-if="!isCreationMode && device" :groups="device.groups" />

            <!-- Usage history (stock-tracked assets only) -->
            <SectionCard v-if="!isCreationMode && device?.quantity != null" content-padding="p-4">
              <template #title>{{ $t('asset-usage-history-heading') }}</template>
              <AssetUsageHistory
                :asset-id="device!.id"
                :unit="device!.unit"
                :current-quantity="device!.quantity"
                @recorded="fetchDeviceData"
              />
            </SectionCard>

            <!-- Plugin panels for device info -->
            <PluginSlot v-if="!isCreationMode && device" slot-name="asset-info-panels" :device="device" />

            <!-- Asset Information (manual devices, edit mode only) -->
            <SectionCard v-if="!isCreationMode && device?.is_editable" content-padding="p-4">
              <template #title>{{ $t('asset-detail-section-device-information') }}</template>

              <div class="flex flex-col gap-4">
                <div class="flex flex-col gap-2">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('asset-detail-field-device-id') }}</h3>
                  <div class="bg-surface-alt rounded-lg p-3 border border-default">
                    <span class="text-primary font-mono text-sm">{{ device.id }}</span>
                  </div>
                </div>

                <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                  <div class="flex flex-col gap-1.5">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('asset-detail-field-created') }}</h4>
                    <p class="text-primary text-sm">{{ formatDateTime(device.created_at) }}</p>
                  </div>
                  <div class="flex flex-col gap-1.5">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('asset-detail-field-last-updated') }}</h4>
                    <p class="text-primary text-sm">{{ formatDateTime(device.updated_at) }}</p>
                  </div>
                </div>

                <div class="pt-4 border-t border-default">
                  <div class="flex items-center gap-2 text-sm">
                    <Icon name="copyMd" size="md" class="text-secondary flex-shrink-0" />
                    <div>
                      <p class="font-medium text-primary">{{ $t('asset-detail-manually-managed') }}</p>
                      <p class="text-xs text-tertiary mt-0.5">{{ $t('asset-detail-manually-managed-description') }}</p>
                    </div>
                  </div>
                </div>
              </div>
            </SectionCard>

            <!-- Externally-synced asset (Intune / Entra). The
                 ID fields and last-sync-time now render through
                 DynamicAttributeForm against the IT baseline
                 attribute schema; this card surfaces the sync
                 source + the unmanage action only. -->
            <SectionCard v-else-if="!isCreationMode && device" content-padding="p-4">
              <template #title>{{ $t('asset-detail-section-external-sync') }}</template>
              <div class="flex flex-col gap-4">
                <div class="flex items-center gap-2 text-sm">
                  <Icon name="refresh" class="text-accent flex-shrink-0" />
                  <div>
                    <p class="font-medium text-primary">
                      {{ $t('asset-detail-external-sync-source', { source: device.external_sync_source || '' }) }}
                    </p>
                    <p class="text-xs text-tertiary mt-0.5">
                      {{ $t('asset-detail-external-sync-note') }}
                    </p>
                  </div>
                </div>
                <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                  <div class="flex flex-col gap-1.5">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('asset-detail-field-created') }}</h4>
                    <p class="text-primary text-sm">{{ formatDateTime(device.created_at) }}</p>
                  </div>
                  <div class="flex flex-col gap-1.5">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('asset-detail-field-last-updated') }}</h4>
                    <p class="text-primary text-sm">{{ formatDateTime(device.updated_at) }}</p>
                  </div>
                </div>
                <div class="pt-4 border-t border-default flex flex-col gap-3">
                  <button
                    @click="handleUnmanageDevice"
                    :disabled="isSaving"
                    class="w-full flex items-center justify-center gap-2 px-4 py-2.5 bg-status-warning/20 text-status-warning rounded-lg hover:bg-status-warning/30 disabled:opacity-50 disabled:cursor-not-allowed transition-colors text-sm font-medium"
                    :title="$t('asset-detail-action-unmanage-title')"
                  >
                    <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                      <path d="M18.84 12.25l1.72-1.71h-.02a5.004 5.004 0 00-.12-7.07 5.006 5.006 0 00-6.95 0l-1.72 1.71" />
                      <path d="M5.17 11.75l-1.71 1.71a5.004 5.004 0 00.12 7.07 5.006 5.006 0 006.95 0l1.71-1.71" />
                      <path d="M8 2v3" /><path d="M2 8h3" /><path d="M16 22v-3" /><path d="M22 16h-3" />
                    </svg>
                    {{ isSaving ? $t('asset-detail-action-unmanage-processing') : $t('asset-detail-action-unmanage') }}
                  </button>
                  <p class="text-xs text-tertiary text-center">{{ $t('asset-detail-unmanage-conversion-note') }}</p>
                </div>
              </div>
            </SectionCard>
          </div>
        </div>

        <!-- Create mode action bar -->
        <div v-if="isCreationMode" class="flex justify-end">
          <div class="flex gap-3">
            <button
              @click="router.push('/assets')"
              :disabled="isSaving"
              class="px-6 py-2.5 bg-surface-alt text-primary rounded-lg hover:bg-surface-hover disabled:opacity-50 transition-colors text-sm font-medium"
            >
              {{ $t('asset-detail-action-cancel') }}
            </button>
            <button
              @click="saveDevice"
              :disabled="isSaving || (!editValues.name && !(attributeDraft.hostname as string | undefined))"
              class="px-6 py-2.5 bg-status-success text-white rounded-lg hover:bg-status-success/80 disabled:opacity-50 disabled:cursor-not-allowed transition-colors text-sm font-medium flex items-center gap-2"
            >
              <Spinner v-if="isSaving" />
              {{ isSaving ? $t('asset-detail-action-create-processing') : $t('asset-detail-action-create') }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Not found -->
    <div v-else class="p-6 text-center text-secondary">
      {{ $t('asset-detail-not-found') }}
    </div>

    <!-- User Selection Modal -->
    <UserSelectionModal
      :show="showUserSelectionModal"
      :currentUserId="isCreationMode ? (selectedUser?.uuid ?? null) : (device?.primary_user_uuid ?? null)"
      @close="showUserSelectionModal = false"
      @select-user="handleUserSelection"
    />

    <!-- Unmanage Asset Confirmation Modal -->
    <Modal
      :show="showUnmanageModal"
      :title="$t('asset-detail-unmanage-modal-title')"
      @close="showUnmanageModal = false"
    >
      <div class="flex flex-col items-center gap-4">
        <div class="mx-auto flex items-center justify-center h-12 w-12 rounded-full bg-status-warning/20">
          <svg class="h-6 w-6 text-status-warning" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M18.84 12.25l1.72-1.71h-.02a5.004 5.004 0 00-.12-7.07 5.006 5.006 0 00-6.95 0l-1.72 1.71" />
            <path d="M5.17 11.75l-1.71 1.71a5.004 5.004 0 00.12 7.07 5.006 5.006 0 006.95 0l1.71-1.71" />
            <path d="M8 2v3" /><path d="M2 8h3" /><path d="M16 22v-3" /><path d="M22 16h-3" />
          </svg>
        </div>

        <h3 class="text-xl font-medium text-primary">{{ $t('asset-detail-unmanage-heading') }}</h3>
        <p
          class="text-sm text-secondary text-center max-w-sm"
          v-html="$t('asset-detail-unmanage-confirm-body', { name: (device?.attributes?.hostname as string | undefined) || device?.name || '' })"
        ></p>
        <p class="text-xs text-tertiary text-center max-w-sm">
          {{ $t('asset-detail-unmanage-confirm-note') }}
        </p>

        <p v-if="unmanageError" class="text-sm text-status-error text-center">
          {{ unmanageError }}
        </p>

        <div class="flex justify-center gap-3 mt-2 w-full">
          <button
            @click="showUnmanageModal = false"
            class="flex-1 px-4 py-2.5 bg-surface text-primary rounded-lg hover:bg-surface-hover transition-colors border border-default"
          >
            {{ $t('asset-detail-action-cancel') }}
          </button>
          <button
            @click="confirmUnmanageDevice"
            :disabled="isSaving"
            class="flex-1 px-4 py-2.5 bg-status-warning text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50"
          >
            {{ isSaving ? $t('asset-detail-action-unmanage-processing') : $t('asset-detail-unmanage-action-confirm') }}
          </button>
        </div>
      </div>
    </Modal>
  </div>
</template>
