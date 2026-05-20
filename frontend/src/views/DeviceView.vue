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
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';
import UserCard from '@/components/UserCard.vue';
import UserSelectionModal from '@/components/UserSelectionModal.vue';
import DeviceGroups from '@/components/DeviceGroups.vue';
import PluginSlot from '@/plugins/components/PluginSlot.vue';
import Modal from '@/components/Modal.vue';
import { getDeviceById, updateDevice, createDevice, deleteDevice, unmanageDevice } from '@/services/deviceService';
import { assetKindsService, type AssetKind } from '@/services/assetKindsService';
import { useSSEListeners } from '@/composables/useSSEListeners';
import type { DeviceUpdatedEventData, DeviceDeletedEventData } from '@/types/sse';
import { IntuneIcon, EntraIcon } from '@/components/icons';
import type { Device, DeviceFormData } from '@/types/device';
import DynamicAttributeForm from '@/components/assets/DynamicAttributeForm.vue';

const route = useRoute();
const router = useRouter();
const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const emit = defineEmits(['update:device']);

// State
const device = ref<Device | null>(null);
const loading = ref(true);
const error = ref<string | null>(null);
const isSaving = ref(false);
const showUserSelectionModal = ref(false);
const showAdditionalDetails = ref(false);
const showUnmanageModal = ref(false);
const unmanageError = ref<string | null>(null);
const hostnameRef = ref<HTMLInputElement | null>(null);
const selectedUser = ref<{ uuid: string; name: string; email: string; role: string } | null>(null);

const editValues = ref({
  name: '',
  manufacturer: '',
  model: '',
  hostname: '',
  serial_number: '',
  warranty_status: 'Unknown',
  warranty_start_date: '' as string,
  warranty_end_date: '' as string,
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

const warrantyOptions = computed(() => [
  { value: 'Active', label: t('device-detail-warranty-active') },
  { value: 'Warning', label: t('device-detail-warranty-warning') },
  { value: 'Expired', label: t('device-detail-warranty-expired') },
  { value: 'Unknown', label: t('device-detail-warranty-unknown') }
]);

const warrantyStatusLabel = (status: string | undefined) => {
  switch (status) {
    case 'Active': return t('device-detail-warranty-active');
    case 'Warning': return t('device-detail-warranty-warning');
    case 'Expired': return t('device-detail-warranty-expired');
    case 'Unknown': return t('device-detail-warranty-unknown');
    default: return status ?? '';
  }
};

// Data fetching
const fetchDeviceData = async () => {
  try {
    loading.value = true;
    error.value = null;

    if (isCreationMode.value) {
      editValues.value = {
        name: '', manufacturer: '', model: '',
        hostname: '', serial_number: '', warranty_status: 'Unknown',
        warranty_start_date: '', warranty_end_date: '',
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
      error.value = t('device-detail-error-invalid-id');
      loading.value = false;
      return;
    }

    device.value = await getDeviceById(deviceId);
    editValues.value = {
      name: device.value.name,
      manufacturer: device.value.manufacturer || '',
      model: device.value.model,
      hostname: device.value.hostname,
      serial_number: device.value.serial_number,
      warranty_status: device.value.warranty_status,
      warranty_start_date: device.value.warranty_start_date || '',
      warranty_end_date: device.value.warranty_end_date || '',
      purchase_date: device.value.purchase_date || '',
      asset_tag: device.value.asset_tag || '',
    };
    // Hydrate the kind picker and attribute draft from the
    // loaded device so the (currently read-only) display row
    // shows the right slug; edit support for these fields is a
    // follow-up.
    selectedKindSlug.value = device.value.kind ?? 'device';
    attributeDraft.value = { ...(device.value.attributes ?? {}) };
  } catch (e) {
    error.value = t('device-detail-error-load');
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
      editValues.value[field] = (device.value[field as keyof Device] as string) || '';
    }
  } finally {
    isSaving.value = false;
  }
};

// Device creation
const saveDevice = async () => {
  try {
    isSaving.value = true;
    const deviceData: DeviceFormData = {
      name: editValues.value.hostname || editValues.value.name,
      manufacturer: editValues.value.manufacturer,
      model: editValues.value.model,
      hostname: editValues.value.hostname,
      serial_number: editValues.value.serial_number,
      warranty_status: editValues.value.warranty_status,
      warranty_start_date: editValues.value.warranty_start_date || null,
      warranty_end_date: editValues.value.warranty_end_date || null,
      purchase_date: editValues.value.purchase_date || null,
      asset_tag: editValues.value.asset_tag || null,
      primary_user_uuid: selectedUser.value?.uuid || undefined,
      type: 'Other',
      kind: selectedKindSlug.value,
      attributes: attributeDraft.value,
    };
    const newDevice = await createDevice(deviceData);
    router.replace(`/assets/${newDevice.id}`);
  } catch (err) {
    console.error('Error creating device:', err);
    error.value = t('device-detail-error-create');
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

// Device deletion
const handleDeleteDevice = async () => {
  if (!device.value) return;

  try {
    await deleteDevice(device.value.id);
    router.push('/assets');
  } catch (err) {
    console.error('Error deleting device:', err);
    error.value = t('device-detail-error-delete');
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
    unmanageError.value = t('device-detail-error-unmanage');
  } finally {
    isSaving.value = false;
  }
};

// External links
const openInIntune = () => {
  if (device.value?.intune_device_id) {
    window.open(
      `https://intune.microsoft.com/#view/Microsoft_Intune_Devices/DeviceSettingsMenuBlade/~/overview/mdmDeviceId/${device.value.intune_device_id}`,
      '_blank', 'noopener,noreferrer'
    );
  }
};

const openInEntra = () => {
  if (device.value?.entra_device_id) {
    window.open(
      `https://entra.microsoft.com/#view/Microsoft_AAD_Devices/DeviceDetailsMenuBlade/~/Properties/objectId/${device.value.entra_device_id}`,
      '_blank', 'noopener,noreferrer'
    );
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

on('device-updated', (data) => {
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

on('device-deleted', (data) => {
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
            :label="$t('device-detail-back-to-ticket', { id: fromTicket })"
          />
          <BackButton v-else fallbackRoute="/assets" :label="$t('device-detail-back-to-devices')" />

          <div v-if="isSynced" class="flex items-center gap-2 text-sm">
            <div class="w-2 h-2 rounded-full bg-accent"></div>
            <span class="text-secondary">{{ $t('device-detail-readonly') }}</span>
          </div>
        </div>

        <DeleteButton
          v-if="!isCreationMode && device?.is_editable"
          fallbackRoute="/assets"
          :itemName="$t('device-detail-delete-item-name')"
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
          <template #title>{{ $t('device-detail-section-kind') }}</template>
          <div class="flex flex-col gap-4">
            <div class="flex flex-col gap-1.5">
              <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">
                {{ $t('device-detail-field-kind') }}
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
          <!-- Left column: Device Details -->
          <SectionCard content-padding="p-4">
            <template #title>{{ $t('device-detail-section-details') }}</template>

            <div class="flex flex-col gap-4">
              <!-- Name -->
              <div class="flex flex-col gap-1.5">
                <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('device-detail-field-name') }}</h3>
                <input
                  v-if="isCreationMode"
                  v-model="editValues.name"
                  type="text"
                  :placeholder="$t('device-detail-field-name-placeholder-create')"
                  class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                />
                <InlineEdit
                  v-else
                  v-model="editValues.name"
                  :placeholder="$t('device-detail-field-name-placeholder-edit')"
                  text-size="sm"
                  :can-edit="device?.is_editable ?? false"
                  @update:modelValue="() => saveField('name')"
                />
              </div>

              <!-- Hostname -->
              <div class="flex flex-col gap-1.5">
                <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('device-detail-field-hostname') }}</h3>
                <input
                  v-if="isCreationMode"
                  ref="hostnameRef"
                  v-model="editValues.hostname"
                  type="text"
                  :placeholder="$t('device-detail-field-hostname-placeholder-create')"
                  class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                />
                <InlineEdit
                  v-else
                  v-model="editValues.hostname"
                  :placeholder="device?.hostname || $t('device-detail-field-hostname-placeholder-edit')"
                  text-size="sm"
                  :can-edit="device?.is_editable ?? false"
                  @update:modelValue="() => saveField('hostname')"
                />
              </div>

              <!-- Serial Number -->
              <div class="flex flex-col gap-1.5">
                <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('device-detail-field-serial') }}</h3>
                <input
                  v-if="isCreationMode"
                  v-model="editValues.serial_number"
                  type="text"
                  :placeholder="$t('device-detail-field-serial-placeholder-create')"
                  class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                />
                <InlineEdit
                  v-else
                  v-model="editValues.serial_number"
                  :placeholder="device?.serial_number || $t('device-detail-field-serial-placeholder-edit')"
                  text-size="sm"
                  :can-edit="device?.is_editable ?? false"
                  @update:modelValue="() => saveField('serial_number')"
                />
              </div>

              <!-- Manufacturer + Model side-by-side -->
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-4 pt-2 border-t border-default">
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('device-detail-field-manufacturer') }}</h3>
                  <input
                    v-if="isCreationMode"
                    v-model="editValues.manufacturer"
                    type="text"
                    :placeholder="$t('device-detail-field-manufacturer-placeholder-create')"
                    class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                  />
                  <InlineEdit
                    v-else
                    v-model="editValues.manufacturer"
                    :placeholder="device?.manufacturer || $t('device-detail-field-manufacturer-placeholder-edit')"
                    text-size="sm"
                    :can-edit="device?.is_editable ?? false"
                    @update:modelValue="() => saveField('manufacturer')"
                  />
                </div>

                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('device-detail-field-model') }}</h3>
                  <input
                    v-if="isCreationMode"
                    v-model="editValues.model"
                    type="text"
                    :placeholder="$t('device-detail-field-model-placeholder-create')"
                    class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                  />
                  <InlineEdit
                    v-else
                    v-model="editValues.model"
                    :placeholder="device?.model || $t('device-detail-field-model-placeholder-edit')"
                    text-size="sm"
                    :can-edit="device?.is_editable ?? false"
                    @update:modelValue="() => saveField('model')"
                  />
                </div>
              </div>

              <!-- Warranty Status -->
              <div class="flex flex-col gap-1.5 pt-2 border-t border-default">
                <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('device-detail-field-warranty-status') }}</h3>
                <BaseDropdown
                  v-if="isCreationMode || device?.is_editable"
                  v-model="editValues.warranty_status"
                  :options="warrantyOptions"
                  size="sm"
                  @update:modelValue="() => { if (!isCreationMode) saveField('warranty_status') }"
                />
                <div
                  v-else
                  class="inline-flex items-center px-3 py-2 rounded-lg text-sm font-medium w-fit"
                  :class="{
                    'bg-status-success/30 text-status-success border border-status-success/30': device?.warranty_status === 'Active',
                    'bg-status-warning/30 text-status-warning border border-status-warning/30': device?.warranty_status === 'Warning',
                    'bg-status-error/30 text-status-error border border-status-error/30': device?.warranty_status === 'Expired',
                    'bg-surface-alt text-secondary border border-default': device?.warranty_status === 'Unknown'
                  }"
                >
                  {{ warrantyStatusLabel(device?.warranty_status) }}
                </div>
              </div>

              <!-- Warranty Dates -->
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('device-detail-field-warranty-start') }}</h3>
                  <input
                    v-if="isCreationMode || device?.is_editable"
                    v-model="editValues.warranty_start_date"
                    type="date"
                    class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2 text-primary focus:outline-none focus:ring-2 focus:ring-accent/50 text-sm"
                    @change="() => { if (!isCreationMode) saveField('warranty_start_date') }"
                  />
                  <p v-else class="text-primary text-sm">{{ device?.warranty_start_date || '-' }}</p>
                </div>
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('device-detail-field-warranty-end') }}</h3>
                  <input
                    v-if="isCreationMode || device?.is_editable"
                    v-model="editValues.warranty_end_date"
                    type="date"
                    class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2 text-primary focus:outline-none focus:ring-2 focus:ring-accent/50 text-sm"
                    @change="() => { if (!isCreationMode) saveField('warranty_end_date') }"
                  />
                  <p v-else class="text-primary text-sm">{{ device?.warranty_end_date || '-' }}</p>
                </div>
              </div>

              <!-- Purchase Date + Asset Tag -->
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('device-detail-field-purchase-date') }}</h3>
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
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('device-detail-field-asset-tag') }}</h3>
                  <input
                    v-if="isCreationMode"
                    v-model="editValues.asset_tag"
                    type="text"
                    :placeholder="$t('device-detail-field-asset-tag-placeholder-create')"
                    class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50 text-sm"
                  />
                  <InlineEdit
                    v-else
                    v-model="editValues.asset_tag"
                    :placeholder="$t('device-detail-field-asset-tag-placeholder-edit')"
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
              <template #title>{{ $t('device-detail-section-primary-user') }}</template>

              <div v-if="selectedUser" class="flex flex-col gap-4">
                <UserCard :user="selectedUser" avatar-size="lg" />

                <button
                  @click="showUserSelectionModal = true"
                  class="w-full px-4 py-2.5 bg-accent text-white rounded-lg hover:bg-accent/80 transition-colors text-sm font-medium flex items-center justify-center gap-2"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 7h12m0 0l-4-4m4 4l-4 4m0 6H4m0 0l4 4m-4-4l4-4" />
                  </svg>
                  {{ $t('device-detail-action-change-user') }}
                </button>
              </div>

              <div v-else class="flex flex-col items-center py-8 gap-4">
                <div class="inline-flex items-center justify-center w-12 h-12 bg-surface-alt rounded-full">
                  <Icon name="user" size="md" class="text-secondary" />
                </div>
                <p class="text-secondary text-sm">{{ $t('device-detail-no-user-assigned') }}</p>

                <button
                  @click="showUserSelectionModal = true"
                  class="px-4 py-2.5 bg-accent text-white rounded-lg hover:bg-accent/80 transition-colors text-sm font-medium flex items-center gap-2"
                >
                  <Icon name="add" />
                  {{ $t('device-detail-action-assign-user') }}
                </button>
              </div>
            </SectionCard>

            <!-- Primary User (edit mode) -->
            <SectionCard v-if="!isCreationMode && device" content-padding="p-4">
              <template #title>{{ $t('device-detail-section-primary-user') }}</template>

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
                  {{ $t('device-detail-action-change-user') }}
                </button>
              </div>

              <div v-else class="flex flex-col items-center py-8 gap-4">
                <div class="inline-flex items-center justify-center w-12 h-12 bg-surface-alt rounded-full">
                  <Icon name="user" size="md" class="text-secondary" />
                </div>
                <p class="text-secondary text-sm">{{ $t('device-detail-no-user-assigned') }}</p>

                <button
                  v-if="device.is_editable"
                  @click="showUserSelectionModal = true"
                  class="px-4 py-2.5 bg-accent text-white rounded-lg hover:bg-accent/80 transition-colors text-sm font-medium flex items-center gap-2"
                >
                  <Icon name="add" />
                  {{ $t('device-detail-action-assign-user') }}
                </button>
              </div>
            </SectionCard>

            <!-- Groups (edit mode only) -->
            <DeviceGroups v-if="!isCreationMode && device" :groups="device.groups" />

            <!-- Plugin panels for device info -->
            <PluginSlot v-if="!isCreationMode && device" slot-name="device-info-panels" :device="device" />

            <!-- Device Information (manual devices, edit mode only) -->
            <SectionCard v-if="!isCreationMode && device?.is_editable" content-padding="p-4">
              <template #title>{{ $t('device-detail-section-device-information') }}</template>

              <div class="flex flex-col gap-4">
                <div class="flex flex-col gap-2">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('device-detail-field-device-id') }}</h3>
                  <div class="bg-surface-alt rounded-lg p-3 border border-default">
                    <span class="text-primary font-mono text-sm">{{ device.id }}</span>
                  </div>
                </div>

                <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                  <div class="flex flex-col gap-1.5">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('device-detail-field-created') }}</h4>
                    <p class="text-primary text-sm">{{ formatDateTime(device.created_at) }}</p>
                  </div>
                  <div class="flex flex-col gap-1.5">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('device-detail-field-last-updated') }}</h4>
                    <p class="text-primary text-sm">{{ formatDateTime(device.updated_at) }}</p>
                  </div>
                </div>

                <div class="pt-4 border-t border-default">
                  <div class="flex items-center gap-2 text-sm">
                    <Icon name="copyMd" size="md" class="text-secondary flex-shrink-0" />
                    <div>
                      <p class="font-medium text-primary">{{ $t('device-detail-manually-managed') }}</p>
                      <p class="text-xs text-tertiary mt-0.5">{{ $t('device-detail-manually-managed-description') }}</p>
                    </div>
                  </div>
                </div>
              </div>
            </SectionCard>

            <!-- Microsoft Integration (synced devices) -->
            <SectionCard v-else-if="!isCreationMode && device" content-padding="p-4">
              <template #title>{{ $t('device-detail-section-microsoft-integration') }}</template>

              <div class="flex flex-col gap-6">
                <!-- Timestamps -->
                <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                  <div class="flex flex-col gap-1.5">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('device-detail-field-created') }}</h4>
                    <p class="text-primary text-sm">{{ formatDateTime(device.created_at) }}</p>
                  </div>
                  <div class="flex flex-col gap-1.5">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('device-detail-field-last-updated') }}</h4>
                    <p class="text-primary text-sm">{{ formatDateTime(device.updated_at) }}</p>
                  </div>
                </div>

                <!-- Last Sync Time -->
                <div v-if="device.last_sync_time" class="flex flex-col gap-2 pt-4 border-t border-default">
                  <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('device-detail-field-last-intune-check-in') }}</h4>
                  <div class="flex items-center gap-2">
                    <Icon name="refresh" class="text-accent flex-shrink-0" />
                    <p class="text-primary text-sm">{{ formatDateTime(device.last_sync_time) }}</p>
                  </div>
                </div>

                <!-- External Links -->
                <div class="flex flex-col gap-4 pt-4 border-t border-default">
                  <div class="flex flex-wrap gap-3">
                    <button
                      v-if="device.intune_device_id"
                      @click="openInIntune"
                      class="flex-1 min-w-[160px] flex items-center justify-center gap-2 px-4 py-2.5 bg-accent text-white rounded-lg hover:bg-accent/80 transition-colors text-sm font-medium"
                    >
                      <IntuneIcon size="16" class="text-white flex-shrink-0" />
                      <span>{{ $t('device-detail-action-view-in-intune') }}</span>
                      <Icon name="openExternal" class="flex-shrink-0 opacity-70" />
                    </button>

                    <button
                      v-if="device.entra_device_id"
                      @click="openInEntra"
                      class="flex-1 min-w-[160px] flex items-center justify-center gap-2 px-4 py-2.5 bg-surface-alt text-primary rounded-lg hover:bg-surface-hover transition-colors text-sm font-medium border border-default"
                    >
                      <EntraIcon size="16" class="flex-shrink-0" />
                      <span>{{ $t('device-detail-action-view-in-entra') }}</span>
                      <Icon name="openExternal" class="flex-shrink-0 opacity-70" />
                    </button>
                  </div>

                  <!-- Unmanage Button -->
                  <div class="pt-4 border-t border-default flex flex-col gap-3">
                    <button
                      @click="handleUnmanageDevice"
                      :disabled="isSaving"
                      class="w-full flex items-center justify-center gap-2 px-4 py-2.5 bg-status-warning/20 text-status-warning rounded-lg hover:bg-status-warning/30 disabled:opacity-50 disabled:cursor-not-allowed transition-colors text-sm font-medium"
                      :title="$t('device-detail-action-unmanage-title')"
                    >
                      <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M18.84 12.25l1.72-1.71h-.02a5.004 5.004 0 00-.12-7.07 5.006 5.006 0 00-6.95 0l-1.72 1.71" />
                        <path d="M5.17 11.75l-1.71 1.71a5.004 5.004 0 00.12 7.07 5.006 5.006 0 006.95 0l1.71-1.71" />
                        <path d="M8 2v3" /><path d="M2 8h3" /><path d="M16 22v-3" /><path d="M22 16h-3" />
                      </svg>
                      {{ isSaving ? $t('device-detail-action-unmanage-processing') : $t('device-detail-action-unmanage') }}
                    </button>
                    <p class="text-xs text-tertiary text-center">{{ $t('device-detail-unmanage-conversion-note') }}</p>
                  </div>

                  <!-- Technical Details Dropdown -->
                  <div class="pt-4 border-t border-default">
                    <button
                      @click="showAdditionalDetails = !showAdditionalDetails"
                      class="w-full flex items-center justify-between text-secondary hover:text-primary transition-colors text-sm"
                    >
                      <span class="font-medium">{{ showAdditionalDetails ? $t('device-detail-tech-details-hide') : $t('device-detail-tech-details-show') }}</span>
                      <Icon
                        name="chevronDown"
                        class="transition-transform duration-200"
                        :class="{ 'rotate-180': showAdditionalDetails }"
                      />
                    </button>

                    <div v-show="showAdditionalDetails" class="mt-4 divide-y divide-default">
                      <div class="flex items-center justify-between py-2.5">
                        <span class="text-sm text-secondary">{{ $t('device-detail-field-device-id') }}</span>
                        <span class="text-sm text-primary font-mono">{{ device.id }}</span>
                      </div>
                      <div v-if="device.intune_device_id" class="flex items-start justify-between gap-4 py-2.5">
                        <span class="text-sm text-secondary flex-shrink-0">{{ $t('device-detail-field-intune-id') }}</span>
                        <span class="text-sm text-primary font-mono text-right break-all">{{ device.intune_device_id }}</span>
                      </div>
                      <div v-if="device.entra_device_id" class="flex items-start justify-between gap-4 py-2.5">
                        <span class="text-sm text-secondary flex-shrink-0">{{ $t('device-detail-field-entra-id') }}</span>
                        <span class="text-sm text-primary font-mono text-right break-all">{{ device.entra_device_id }}</span>
                      </div>
                    </div>
                  </div>
                </div>

                <!-- No Management IDs fallback -->
                <div v-if="!device.intune_device_id && !device.entra_device_id" class="text-center py-8">
                  <div class="inline-flex items-center justify-center w-12 h-12 bg-surface-alt rounded-full mb-4">
                    <Icon name="checkCircle" size="md" class="text-secondary" />
                  </div>
                  <p class="text-secondary text-sm">{{ $t('device-detail-not-managed-by-intune') }}</p>
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
              {{ $t('device-detail-action-cancel') }}
            </button>
            <button
              @click="saveDevice"
              :disabled="isSaving || !editValues.hostname"
              class="px-6 py-2.5 bg-status-success text-white rounded-lg hover:bg-status-success/80 disabled:opacity-50 disabled:cursor-not-allowed transition-colors text-sm font-medium flex items-center gap-2"
            >
              <Spinner v-if="isSaving" />
              {{ isSaving ? $t('device-detail-action-create-processing') : $t('device-detail-action-create') }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Not found -->
    <div v-else class="p-6 text-center text-secondary">
      {{ $t('device-detail-not-found') }}
    </div>

    <!-- User Selection Modal -->
    <UserSelectionModal
      :show="showUserSelectionModal"
      :currentUserId="isCreationMode ? (selectedUser?.uuid ?? null) : (device?.primary_user_uuid ?? null)"
      @close="showUserSelectionModal = false"
      @select-user="handleUserSelection"
    />

    <!-- Unmanage Device Confirmation Modal -->
    <Modal
      :show="showUnmanageModal"
      :title="$t('device-detail-unmanage-modal-title')"
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

        <h3 class="text-xl font-medium text-primary">{{ $t('device-detail-unmanage-heading') }}</h3>
        <p
          class="text-sm text-secondary text-center max-w-sm"
          v-html="$t('device-detail-unmanage-confirm-body', { name: device?.hostname || device?.name || '' })"
        ></p>
        <p class="text-xs text-tertiary text-center max-w-sm">
          {{ $t('device-detail-unmanage-confirm-note') }}
        </p>

        <p v-if="unmanageError" class="text-sm text-status-error text-center">
          {{ unmanageError }}
        </p>

        <div class="flex justify-center gap-3 mt-2 w-full">
          <button
            @click="showUnmanageModal = false"
            class="flex-1 px-4 py-2.5 bg-surface text-primary rounded-lg hover:bg-surface-hover transition-colors border border-default"
          >
            {{ $t('device-detail-action-cancel') }}
          </button>
          <button
            @click="confirmUnmanageDevice"
            :disabled="isSaving"
            class="flex-1 px-4 py-2.5 bg-status-warning text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50"
          >
            {{ isSaving ? $t('device-detail-action-unmanage-processing') : $t('device-detail-unmanage-action-confirm') }}
          </button>
        </div>
      </div>
    </Modal>
  </div>
</template>
