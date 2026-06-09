<script setup lang="ts">
import { ref, computed, watch, onMounted, nextTick } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { formatDateTime } from '@/utils/dateUtils';
import BackButton from '@/components/common/BackButton.vue';
import SearchableDropdown, { type DropdownOption } from '@/components/common/SearchableDropdown.vue';
import Button from '@/components/common/Button.vue';
import DeleteButton from '@/components/common/DeleteButton.vue';
import FormInput from '@/components/common/FormInput.vue';
import InlineEdit from '@/components/common/InlineEdit.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import DatePicker from '@/components/common/DatePicker.vue';
import { extractErrorMessage } from '@/utils/errors';
import Icon from '@/components/common/Icon.vue';
import UserCard from '@/components/UserCard.vue';
import UserSelectionModal from '@/components/UserSelectionModal.vue';
import DeviceGroups from '@/components/AssetGroups.vue';
import AssetMediaPanel from '@/components/assets/AssetMediaPanel.vue';
import AssetLifecyclePanel from '@/components/assets/AssetLifecyclePanel.vue';
import AssetStatusBadge from '@/components/assets/AssetStatusBadge.vue';
import AssetUsageHistory from '@/components/assets/AssetUsageHistory.vue';
import PluginSlot from '@/plugins/components/PluginSlot.vue';
import Modal from '@/components/Modal.vue';
import { getAssetById, updateAsset, createAsset, deleteAsset, unmanageAsset } from '@/services/assetService';
import { type AssetKind } from '@/services/assetKindsService';
import { useAssetKindsQuery } from '@/composables/useAssetKindsQuery';
import { useAssetLocationsQuery } from '@/composables/useAssetLocationsQuery';
import { useSyncActions } from '@/composables/useSyncActions';
import { useAuthStore } from '@/stores/auth';
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
const { locations: knownLocations } = useAssetLocationsQuery();

const editValues = ref({
  name: '',
  manufacturer: '',
  model: '',
  serial_number: '',
  location: '',
  purchase_date: '' as string,
  asset_tag: '' as string,
  quantity: '' as string,
  unit: '' as string,
  low_stock_threshold: '' as string,
});

// Asset-kind registry state. Loaded eagerly on mount so the
// picker is populated before the create form renders; in edit
// mode it backs the read-only "Kind" row at the bottom of the
// details card.
// Shared Pinia Colada cache so an admin edit on /admin/asset-kinds
// invalidates this picker without us having to manually re-fetch.
// Cold-start (no cached data) falls back to a single hard-coded
// 'device' option below via the same defensive guard the previous
// onMounted+try/catch had.
const { kinds: kindsRef, error: kindsError } = useAssetKindsQuery();
const kinds = computed<AssetKind[]>(() => kindsRef.value);
const selectedKindSlug = ref<string>('device');
const attributeDraft = ref<Record<string, unknown>>({});

const selectedKind = computed<AssetKind | null>(
  () => kinds.value.find((k) => k.slug === selectedKindSlug.value) ?? null,
);

const selectedKindCategory = computed(() => selectedKind.value?.category ?? 'generic');
const canHavePrimaryUser = computed(() =>
  selectedKindCategory.value === 'it' || selectedKindCategory.value === 'logical',
);
const showPrimaryUserPanel = computed(() =>
  canHavePrimaryUser.value || (!isCreationMode.value && Boolean(device.value?.primary_user)),
);
const showCreateInventoryPanel = computed(
  () =>
    selectedKindCategory.value === 'bulk' ||
    editValues.value.quantity.trim() !== '' ||
    editValues.value.unit.trim() !== '' ||
    editValues.value.low_stock_threshold.trim() !== '',
);

const kindOptions = computed<DropdownOption[]>(() =>
  kinds.value.map((k) => ({
    value: k.slug,
    label: k.label,
    description: k.description ?? undefined,
    icon: k.icon ?? undefined,
  })),
);

const selectedKindSchema = computed(
  () => (selectedKind.value?.attribute_schema as Record<string, unknown>) ?? null,
);

// kinds are auto-fetched by useAssetKindsQuery; on error the
// computed `kinds` ref stays empty and the picker falls back to
// the static 'device' default below, preserving the previous
// "non-fatal for non-admins / endpoint outage" behaviour.
watch(kindsError, (err) => {
  if (err) {
    console.warn('asset-kinds list failed; defaulting to device only', err);
  }
});

// Computed
const isCreationMode = computed(() => !route.params.id || route.params.id === 'new');

const fromTicket = computed(() =>
  route.query.fromTicket ? Number(route.query.fromTicket) : null
);

const isSynced = computed(() => device.value != null && !device.value.is_editable);
const displayName = computed(() => {
  if (isCreationMode.value) {
    return editValues.value.name || (attributeDraft.value.hostname as string | undefined) || '';
  }
  if (!device.value) return '';
  return device.value.name || (device.value.attributes?.hostname as string | undefined) || `#${device.value.id}`;
});

const displaySubtitle = computed(() => {
  const parts = [
    selectedKind.value?.label ?? selectedKindSlug.value,
    editValues.value.model || device.value?.model,
    editValues.value.serial_number || device.value?.serial_number,
  ].filter(Boolean);
  return parts.join(' · ');
});

const stockStatus = computed<'none' | 'tracked' | 'low'>(() => {
  const quantity = isCreationMode.value ? editValues.value.quantity : (device.value?.quantity ?? '');
  if (!quantity) return 'none';
  return isLowStock.value ? 'low' : 'tracked';
});

const stockStatusLabel = computed(() => {
  if (stockStatus.value === 'low') return t('assets-list-low-stock-badge');
  if (stockStatus.value === 'tracked') {
    const quantity = isCreationMode.value ? editValues.value.quantity : (device.value?.quantity ?? '');
    const unit = isCreationMode.value ? editValues.value.unit : (device.value?.unit ?? '');
    return unit ? `${quantity} ${unit}` : quantity;
  }
  return t('asset-detail-stock-not-tracked');
});

const locationSuggestions = computed(() => {
  const query = editValues.value.location.trim().toLowerCase();
  return knownLocations.value
    .filter((entry) => {
      const location = entry.location.trim();
      if (!location) return false;
      if (!query) return true;
      return location.toLowerCase().includes(query) && location.toLowerCase() !== query;
    })
    .slice(0, 5);
});

const managementLabel = computed(() => {
  if (isCreationMode.value) return t('asset-detail-manually-managed');
  if (isSynced.value) {
    return t('asset-detail-external-sync-source', { source: device.value?.external_sync_source || '' });
  }
  return t('asset-detail-manually-managed');
});

watch(
  canHavePrimaryUser,
  (canAssign) => {
    if (isCreationMode.value && !canAssign) {
      selectedUser.value = null;
    }
  },
);

/** Kind picker + attribute form are editable in creation mode
 *  for any user, and in edit mode only when the row isn't owned
 *  by an external sync. Synced rows (Intune, Entra) stay locked
 *  because the next sync run would overwrite any manual edit. */
const isKindOrAttributesEditable = computed(
  () => isCreationMode.value || (device.value?.is_editable ?? false),
);

/** Attribute draft drifts from the saved value as the admin
 *  types in the dynamic form. Comparing the JSON serialisation
 *  is fine for the attribute payloads we ship (small,
 *  deterministic key ordering since the form rebuilds the
 *  object from the schema each time). */
const attributesDirty = computed(() => {
  if (!device.value) return false;
  return (
    JSON.stringify(attributeDraft.value) !==
    JSON.stringify(device.value.attributes ?? {})
  );
});

const kindChangeError = ref<string | null>(null);
const attributesError = ref<string | null>(null);

// Kind change is deferred to an explicit confirm dialog (it clears the
// row's attributes), so the picker change just stages the target slug.
const showKindChangeConfirm = ref(false);
const pendingKindSlug = ref<string | null>(null);
const kindChangeConfirmMessage = computed(() =>
  t('asset-detail-kind-change-confirm', {
    newKind:
      kinds.value.find((k) => k.slug === pendingKindSlug.value)?.label ??
      pendingKindSlug.value ??
      '',
  }),
);

/** Persist the attribute draft against the existing kind. Used
 *  by the Save attributes button in edit mode. */
async function saveAttributes() {
  if (!device.value || !attributesDirty.value) return;
  isSaving.value = true;
  attributesError.value = null;
  try {
    const updated = await updateAsset(device.value.id, {
      attributes: attributeDraft.value,
    });
    device.value = { ...device.value, ...updated };
    attributeDraft.value = { ...(updated.attributes ?? {}) };
  } catch (err) {
    attributesError.value = extractErrorMessage(err, t('asset-detail-attributes-save-failed'));
  } finally {
    isSaving.value = false;
  }
}

function discardAttributes() {
  if (!device.value) return;
  attributeDraft.value = { ...(device.value.attributes ?? {}) };
  attributesError.value = null;
}

function scrollToInventory() {
  document
    .getElementById('asset-inventory-panel')
    ?.scrollIntoView({ behavior: 'smooth', block: 'start' });
}

/** Kind change is a bigger commit than a single field update:
 *  it almost certainly invalidates the row's current attributes
 *  (different schemas, different required keys). We require an
 *  explicit confirm, clear attributes to {} on save, and let
 *  the admin re-enter them against the new kind's schema. */
function onKindPickerChange() {
  if (!device.value) return;
  if (isCreationMode.value) {
    // Creation flow: kind/attributes get sent together on
    // saveDevice; no per-change save here.
    return;
  }
  const newSlug = selectedKindSlug.value;
  const currentSlug = device.value.kind ?? 'generic';
  if (newSlug === currentSlug) return;

  // Stage the change and defer to the confirm dialog.
  pendingKindSlug.value = newSlug;
  showKindChangeConfirm.value = true;
}

function cancelKindChange() {
  showKindChangeConfirm.value = false;
  pendingKindSlug.value = null;
  // Revert the picker to the persisted kind.
  selectedKindSlug.value = device.value?.kind ?? 'generic';
}

async function confirmKindChange() {
  showKindChangeConfirm.value = false;
  const newSlug = pendingKindSlug.value;
  pendingKindSlug.value = null;
  if (!device.value || !newSlug) return;

  isSaving.value = true;
  kindChangeError.value = null;
  try {
    const updated = await updateAsset(device.value.id, {
      kind: newSlug,
      attributes: {},
    });
    device.value = { ...device.value, ...updated };
    attributeDraft.value = { ...(updated.attributes ?? {}) };
  } catch (err) {
    kindChangeError.value = extractErrorMessage(err, t('asset-detail-kind-change-failed'));
    selectedKindSlug.value = device.value.kind ?? 'generic';
  } finally {
    isSaving.value = false;
  }
}

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
        quantity: '', unit: '', low_stock_threshold: '',
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

    device.value = await getAssetById(deviceId);
    editValues.value = {
      name: device.value.name,
      manufacturer: device.value.manufacturer || '',
      model: device.value.model,
      serial_number: device.value.serial_number,
      location: device.value.location || '',
      purchase_date: device.value.purchase_date || '',
      asset_tag: device.value.asset_tag || '',
      quantity: device.value.quantity ?? '',
      unit: device.value.unit ?? '',
      low_stock_threshold: device.value.low_stock_threshold ?? '',
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

async function selectLocationSuggestion(location: string) {
  editValues.value.location = location;
  if (!isCreationMode.value) {
    await saveField('location');
  }
}

// Field saving (edit mode)
const saveField = async (field: keyof typeof editValues.value) => {
  if (!device.value) return;

  try {
    isSaving.value = true;
    const updatedDevice = await updateAsset(device.value.id, {
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

/** Inline-edit a stock-tracking field (quantity, unit, or
 *  low_stock_threshold). Empty input is omitted from the PATCH
 *  body so the backend leaves the column untouched, since the
 *  Diesel AsChangeset can't distinguish "absent" from "null"
 *  without a serde double-Option helper. Clearing a value back
 *  to NULL isn't supported through this surface; admins who need
 *  that should use the API directly. */
const saveStockField = async (field: 'quantity' | 'unit' | 'low_stock_threshold') => {
  if (!device.value) return;
  const raw = editValues.value[field].trim();
  if (raw === '') {
    editValues.value[field] = (device.value[field] as string | null | undefined) ?? '';
    return;
  }
  try {
    isSaving.value = true;
    const updatedDevice = await updateAsset(device.value.id, { [field]: raw });
    device.value = { ...device.value, ...updatedDevice };
  } catch (err) {
    console.error('Error saving stock field:', err);
    editValues.value[field] = (device.value[field] as string | null | undefined) ?? '';
  } finally {
    isSaving.value = false;
  }
};

/** Derived low-stock flag for the warning row on AssetView. The
 *  backend is the source of truth for the SSE crossing event;
 *  this is a UI-only paint, OK to use parseFloat as a quick
 *  comparison since both strings come from the same NUMERIC(12,3)
 *  column. */
const isLowStock = computed(() => {
  const q = isCreationMode.value ? editValues.value.quantity : device.value?.quantity;
  const t = isCreationMode.value ? editValues.value.low_stock_threshold : device.value?.low_stock_threshold;
  if (q == null || t == null) return false;
  if (q === '' || t === '') return false;
  return parseFloat(q) <= parseFloat(t);
});

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
      quantity: editValues.value.quantity.trim() || null,
      unit: editValues.value.unit.trim() || null,
      low_stock_threshold: editValues.value.low_stock_threshold.trim() || null,
    };
    const newDevice = await createAsset(deviceData);
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
    const updatedDevice = await updateAsset(device.value.id, {
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
    await deleteAsset(device.value.id);
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
    const updatedDevice = await unmanageAsset(device.value.id);
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

// Real-time updates via the sync-action stream (cross-machine). The
// detail view edits the full asset DTO, of which the sync payload is a
// subset, so on a remote change we refetch the whole row rather than
// patch fields. Skip actions authored by the current user — their own
// edits are already applied locally by the save handlers (this is the
// echo suppression the discrete SSE did via source_client_id).
const auth = useAuthStore();
const canChangeLifecycle = computed(() => auth.isTechnician);
useSyncActions(
  (actions) => {
    const id = device.value?.id;
    if (id == null) return;
    const mine = auth.user?.uuid ?? null;
    const relevant = actions.filter(
      (a) => a.aggregate_id === String(id) && a.actor_uuid !== mine,
    );
    if (relevant.length === 0) return;
    // A delete of the open asset: leave the now-gone detail view.
    if (relevant.some((a) => a.op === 'D')) {
      router.push('/assets');
      return;
    }
    void fetchDeviceData();
  },
  { aggregates: ['asset'], debounceMs: 300 },
);

// Lifecycle
onMounted(() => {
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
      <!-- Content area -->
      <div class="flex flex-col gap-6 px-6 py-4 mx-auto w-full max-w-8xl">
        <AlertMessage v-if="error" type="error" :message="error" />

        <section class="bg-surface rounded-xl border border-default overflow-hidden">
          <div class="p-4 sm:p-5 flex flex-col gap-4">
            <div class="flex flex-col sm:flex-row sm:items-start sm:justify-between gap-4">
              <div class="flex flex-col gap-3 min-w-0">
                <BackButton
                  v-if="fromTicket"
                  :fallbackRoute="`/tickets/${fromTicket}`"
                  :label="$t('asset-detail-back-to-ticket', { id: fromTicket })"
                  compact
                />
                <BackButton v-else fallbackRoute="/assets" :label="$t('asset-detail-back-to-devices')" compact />

                <div class="flex items-start gap-3 min-w-0">
                  <div class="w-11 h-11 rounded-lg bg-surface-alt border border-default flex items-center justify-center flex-shrink-0">
                    <Icon name="device" class="text-secondary" />
                  </div>
                  <div class="min-w-0">
                    <div class="flex flex-wrap items-center gap-2">
                      <h1 v-if="displayName" class="text-xl sm:text-2xl font-semibold text-primary truncate">
                        {{ displayName }}
                      </h1>
                      <span
                        class="inline-flex items-center px-2 py-0.5 rounded-md text-xs font-medium bg-accent/10 text-accent"
                      >
                        {{ selectedKind?.label ?? selectedKindSlug }}
                      </span>
                      <span
                        v-if="isSynced"
                        class="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs font-medium bg-surface-alt text-secondary border border-default"
                      >
                        <Icon name="lock" size="xs" />
                        {{ $t('asset-detail-readonly') }}
                      </span>
                    </div>
                    <p v-if="displaySubtitle" class="mt-1 text-sm text-secondary truncate">
                      {{ displaySubtitle }}
                    </p>
                  </div>
                </div>
              </div>

              <div class="flex items-center gap-2 self-start">
                <Button
                  v-if="!isCreationMode && device?.quantity != null"
                  variant="secondary"
                  icon="history"
                  @click="scrollToInventory"
                >
                  {{ $t('asset-usage-history-heading') }}
                </Button>
                <DeleteButton
                  v-if="!isCreationMode && device?.is_editable"
                  fallbackRoute="/assets"
                  :itemName="$t('asset-detail-delete-item-name')"
                  @delete="handleDeleteDevice"
                />
              </div>
            </div>

            <div class="grid grid-cols-1 sm:grid-cols-3 gap-3">
              <div class="rounded-lg border border-default bg-surface-alt px-3 py-2">
                <p class="text-[11px] font-medium uppercase tracking-wide text-tertiary">
                  {{ canHavePrimaryUser || (!isCreationMode && device?.primary_user) ? $t('asset-detail-section-primary-user') : $t('asset-detail-field-location') }}
                </p>
                <p class="mt-1 text-sm font-medium text-primary truncate">
                  {{
                    canHavePrimaryUser || (!isCreationMode && device?.primary_user)
                      ? (isCreationMode ? (selectedUser?.name ?? $t('asset-detail-no-user-assigned')) : (device?.primary_user?.name ?? $t('asset-detail-no-user-assigned')))
                      : (editValues.location || device?.location || $t('assets-list-grouping-location-none'))
                  }}
                </p>
              </div>
              <div class="rounded-lg border border-default bg-surface-alt px-3 py-2">
                <p class="text-[11px] font-medium uppercase tracking-wide text-tertiary">
                  {{ $t('asset-detail-section-stock') }}
                </p>
                <p
                  class="mt-1 text-sm font-medium truncate"
                  :class="stockStatus === 'low' ? 'text-status-warning' : 'text-primary'"
                >
                  {{ stockStatusLabel }}
                </p>
              </div>
              <div class="rounded-lg border border-default bg-surface-alt px-3 py-2">
                <p class="text-[11px] font-medium uppercase tracking-wide text-tertiary">
                  {{ $t('asset-detail-section-device-information') }}
                </p>
                <div class="mt-1 flex flex-col gap-1.5">
                  <p class="text-sm font-medium text-primary truncate">
                    {{ managementLabel }}
                  </p>
                  <AssetStatusBadge
                    v-if="!isCreationMode && device?.status"
                    :status="device.status"
                  />
                </div>
              </div>
            </div>
          </div>
        </section>

        <!-- Kind picker + dynamic attribute form. In creation
             mode the admin chooses the kind and fills in any
             per-kind attributes; in edit mode for editable
             rows, both the picker and the attribute form remain
             writable. Externally synced rows (Intune, Entra)
             stay read-only because the next sync would
             overwrite manual edits anyway. -->
        <SectionCard v-if="kinds.length > 0" content-padding="p-4">
          <template #title>{{ $t('asset-detail-section-kind') }}</template>
          <div class="flex flex-col gap-4">
            <div class="flex flex-col gap-1.5">
              <SearchableDropdown
                v-if="isKindOrAttributesEditable"
                :model-value="selectedKindSlug"
                :options="kindOptions"
                :label="$t('asset-detail-field-kind')"
                size="sm"
                @update:model-value="(value) => { selectedKindSlug = String(value); onKindPickerChange() }"
              />
              <p v-else class="text-sm text-primary">
                {{ selectedKind?.label ?? selectedKindSlug }}
              </p>
              <p v-if="selectedKind?.description" class="text-xs text-tertiary">
                {{ selectedKind.description }}
              </p>
              <AlertMessage v-if="kindChangeError" type="error" :message="kindChangeError" />
            </div>
            <DynamicAttributeForm
              v-if="selectedKindSchema"
              :schema="selectedKindSchema"
              v-model="attributeDraft"
              :disabled="!isKindOrAttributesEditable"
            />
            <div
              v-if="!isCreationMode && isKindOrAttributesEditable && attributesDirty"
              class="flex items-center gap-2 pt-2"
            >
              <Button
                size="sm"
                :disabled="isSaving"
                @click="saveAttributes"
              >
                {{ $t('asset-detail-attributes-save') }}
              </Button>
              <Button
                size="sm"
                variant="secondary"
                :disabled="isSaving"
                @click="discardAttributes"
              >
                {{ $t('asset-detail-attributes-discard') }}
              </Button>
            </div>
            <AlertMessage v-if="attributesError" type="error" :message="attributesError" />
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
                <FormInput
                  v-if="isCreationMode"
                  v-model="editValues.name"
                  :placeholder="$t('asset-detail-field-name-placeholder-create')"
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
                <FormInput
                  v-if="isCreationMode"
                  v-model="editValues.serial_number"
                  :placeholder="$t('asset-detail-field-serial-placeholder-create')"
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
                  <FormInput
                    v-if="isCreationMode"
                    v-model="editValues.manufacturer"
                    :placeholder="$t('asset-detail-field-manufacturer-placeholder-create')"
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
                  <FormInput
                    v-if="isCreationMode"
                    v-model="editValues.model"
                    :placeholder="$t('asset-detail-field-model-placeholder-create')"
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

              <!-- Purchase date + asset tag + location -->
              <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('asset-detail-field-purchase-date') }}</h3>
                  <DatePicker
                    v-if="isCreationMode || device?.is_editable"
                    v-model="editValues.purchase_date"
                    size="md"
                    block
                    :aria-label="$t('asset-detail-field-purchase-date')"
                    @update:model-value="() => { if (!isCreationMode) saveField('purchase_date') }"
                  />
                  <p v-else class="text-primary text-sm">{{ device?.purchase_date || '-' }}</p>
                </div>
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('asset-detail-field-asset-tag') }}</h3>
                  <FormInput
                    v-if="isCreationMode"
                    v-model="editValues.asset_tag"
                    :placeholder="$t('asset-detail-field-asset-tag-placeholder-create')"
                    size="sm"
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
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">{{ $t('asset-detail-field-location') }}</h3>
                  <FormInput
                    v-if="isCreationMode"
                    v-model="editValues.location"
                    :placeholder="$t('asset-detail-field-location-placeholder-create')"
                    size="sm"
                  />
                  <InlineEdit
                    v-else
                    v-model="editValues.location"
                    :placeholder="$t('asset-detail-field-location-placeholder-edit')"
                    text-size="sm"
                    :can-edit="device?.is_editable ?? false"
                    @update:modelValue="() => saveField('location')"
                  />
                  <div
                    v-if="(isCreationMode || device?.is_editable) && locationSuggestions.length"
                    class="flex flex-wrap items-center gap-1.5"
                  >
                    <span class="text-[11px] text-tertiary">
                      {{ $t('asset-detail-location-suggestions') }}
                    </span>
                    <Button
                      v-for="suggestion in locationSuggestions"
                      :key="suggestion.location"
                      variant="ghost"
                      size="sm"
                      class="!px-2 !py-1 border border-subtle"
                      @click="selectLocationSuggestion(suggestion.location)"
                    >
                      {{ suggestion.location }}
                    </Button>
                  </div>
                </div>
              </div>
            </div>
          </SectionCard>

          <!-- Right column -->
          <div v-if="isCreationMode || device" class="flex flex-col gap-6">
            <SectionCard v-if="isCreationMode && showCreateInventoryPanel" id="asset-inventory-panel" content-padding="p-4">
              <template #title>{{ $t('asset-detail-section-stock') }}</template>

              <div class="flex flex-col gap-4">
                <div v-if="isLowStock" class="flex items-center gap-2 px-3 py-2 bg-status-warning/10 text-status-warning rounded-lg text-sm">
                  <Icon name="warning" />
                  <span>{{ $t('asset-detail-low-stock-warning', { quantity: editValues.quantity, unit: editValues.unit, threshold: editValues.low_stock_threshold }) }}</span>
                </div>

                <FormInput
                  v-model="editValues.quantity"
                  :label="$t('asset-detail-field-quantity')"
                  :placeholder="$t('asset-detail-field-quantity-placeholder')"
                  inputmode="decimal"
                  size="sm"
                />

                <FormInput
                  v-model="editValues.unit"
                  :label="$t('asset-detail-field-unit')"
                  :placeholder="$t('asset-detail-field-unit-placeholder')"
                  size="sm"
                />

                <FormInput
                  v-model="editValues.low_stock_threshold"
                  :label="$t('asset-detail-field-low-stock-threshold')"
                  :placeholder="$t('asset-detail-field-low-stock-threshold-placeholder')"
                  :description="$t('asset-detail-field-low-stock-threshold-help')"
                  inputmode="decimal"
                  size="sm"
                />
              </div>
            </SectionCard>

            <SectionCard v-if="isCreationMode && !canHavePrimaryUser" content-padding="p-4">
              <template #title>{{ $t('asset-detail-section-physical-context') }}</template>

              <div class="flex flex-col gap-3">
                <div class="rounded-lg border border-dashed border-default bg-surface-alt p-4 flex items-start gap-3">
                  <Icon name="paperclip" class="text-tertiary flex-shrink-0 mt-0.5" />
                  <div class="min-w-0">
                    <p class="text-sm font-medium text-primary">
                      {{ $t('asset-detail-photo-placeholder-title') }}
                    </p>
                    <p class="text-xs text-tertiary mt-1">
                      {{ $t('asset-detail-photo-placeholder-description') }}
                    </p>
                  </div>
                </div>
                <p class="text-xs text-tertiary">
                  {{ $t('asset-detail-physical-context-help') }}
                </p>
              </div>
            </SectionCard>

            <!-- Primary User (create mode) -->
            <SectionCard v-if="isCreationMode && canHavePrimaryUser" content-padding="p-4">
              <template #title>{{ $t('asset-detail-section-primary-user') }}</template>

              <div v-if="selectedUser" class="flex flex-col gap-4">
                <UserCard :user="selectedUser" avatar-size="lg" />

                <Button
                  block
                  icon="user"
                  @click="showUserSelectionModal = true"
                >
                  {{ $t('asset-detail-action-change-user') }}
                </Button>
              </div>

              <div v-else class="flex flex-col items-center py-8 gap-4">
                <div class="inline-flex items-center justify-center w-12 h-12 bg-surface-alt rounded-full">
                  <Icon name="user" size="md" class="text-secondary" />
                </div>
                <p class="text-secondary text-sm">{{ $t('asset-detail-no-user-assigned') }}</p>

                <Button
                  icon="add"
                  @click="showUserSelectionModal = true"
                >
                  {{ $t('asset-detail-action-assign-user') }}
                </Button>
              </div>
            </SectionCard>

            <!-- Primary User (edit mode) -->
            <SectionCard v-if="!isCreationMode && device && showPrimaryUserPanel" content-padding="p-4">
              <template #title>{{ $t('asset-detail-section-primary-user') }}</template>

              <div v-if="device.primary_user" class="flex flex-col gap-4">
                <UserCard :user="device.primary_user" avatar-size="lg" />

                <Button
                  v-if="device.is_editable"
                  block
                  icon="user"
                  @click="showUserSelectionModal = true"
                >
                  {{ $t('asset-detail-action-change-user') }}
                </Button>
              </div>

              <div v-else class="flex flex-col items-center py-8 gap-4">
                <div class="inline-flex items-center justify-center w-12 h-12 bg-surface-alt rounded-full">
                  <Icon name="user" size="md" class="text-secondary" />
                </div>
                <p class="text-secondary text-sm">{{ $t('asset-detail-no-user-assigned') }}</p>

                <Button
                  v-if="device.is_editable"
                  icon="add"
                  @click="showUserSelectionModal = true"
                >
                  {{ $t('asset-detail-action-assign-user') }}
                </Button>
              </div>
            </SectionCard>

            <!-- Groups (edit mode only) -->
            <DeviceGroups v-if="!isCreationMode && device" :groups="device.groups" />

            <SectionCard v-if="!isCreationMode && device" content-padding="p-4">
              <template #title>{{ $t('asset-lifecycle-heading') }}</template>
              <AssetLifecyclePanel
                :asset-id="device.id"
                :current-status="device.status"
                :can-edit="canChangeLifecycle"
              />
            </SectionCard>

            <SectionCard v-if="!isCreationMode && device" content-padding="p-4">
              <template #title>{{ $t('asset-media-heading') }}</template>
              <AssetMediaPanel
                :asset-id="device.id"
                :can-edit="device.is_editable"
              />
            </SectionCard>

            <!-- Stock tracking (editable assets only). Surfaces
                 the three columns that drive consumable usage:
                 quantity (on-hand count), unit (label), and the
                 optional low_stock_threshold. -->
            <SectionCard v-if="!isCreationMode && device?.is_editable" id="asset-inventory-panel" content-padding="p-4">
              <template #title>{{ $t('asset-detail-section-stock') }}</template>

              <div class="flex flex-col gap-4">
                <div v-if="isLowStock" class="flex items-center gap-2 px-3 py-2 bg-status-warning/10 text-status-warning rounded-lg text-sm">
                  <Icon name="warning" />
                  <span>{{ $t('asset-detail-low-stock-warning', { quantity: device!.quantity!, unit: device!.unit ?? '', threshold: device!.low_stock_threshold! }) }}</span>
                </div>

                <FormInput
                  v-model="editValues.quantity"
                  :label="$t('asset-detail-field-quantity')"
                  :placeholder="$t('asset-detail-field-quantity-placeholder')"
                  inputmode="decimal"
                  size="sm"
                  @blur="saveStockField('quantity')"
                  @keyup.enter="saveStockField('quantity')"
                />

                <FormInput
                  v-model="editValues.unit"
                  :label="$t('asset-detail-field-unit')"
                  :placeholder="$t('asset-detail-field-unit-placeholder')"
                  size="sm"
                  @blur="saveStockField('unit')"
                  @keyup.enter="saveStockField('unit')"
                />

                <FormInput
                  v-model="editValues.low_stock_threshold"
                  :label="$t('asset-detail-field-low-stock-threshold')"
                  :placeholder="$t('asset-detail-field-low-stock-threshold-placeholder')"
                  :description="$t('asset-detail-field-low-stock-threshold-help')"
                  inputmode="decimal"
                  size="sm"
                  @blur="saveStockField('low_stock_threshold')"
                  @keyup.enter="saveStockField('low_stock_threshold')"
                />
              </div>
            </SectionCard>

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
                  <Button
                    @click="handleUnmanageDevice"
                    :disabled="isSaving"
                    block
                    variant="secondary"
                    icon="refresh"
                    :title="$t('asset-detail-action-unmanage-title')"
                  >
                    {{ isSaving ? $t('asset-detail-action-unmanage-processing') : $t('asset-detail-action-unmanage') }}
                  </Button>
                  <p class="text-xs text-tertiary text-center">{{ $t('asset-detail-unmanage-conversion-note') }}</p>
                </div>
              </div>
            </SectionCard>
          </div>
        </div>

        <!-- Create mode action bar -->
        <div v-if="isCreationMode" class="flex justify-end">
          <div class="flex gap-3">
            <Button
              variant="secondary"
              @click="router.push('/assets')"
              :disabled="isSaving"
            >
              {{ $t('asset-detail-action-cancel') }}
            </Button>
            <Button
              @click="saveDevice"
              :disabled="isSaving || (!editValues.name && !(attributeDraft.hostname as string | undefined))"
              :loading="isSaving"
              icon="add"
            >
              {{ isSaving ? $t('asset-detail-action-create-processing') : $t('asset-detail-action-create') }}
            </Button>
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

        <AlertMessage v-if="unmanageError" type="error" :message="unmanageError" />

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

    <!-- Kind change: explicit confirm (clears the row's attributes) -->
    <ConfirmModal
      :show="showKindChangeConfirm"
      variant="warning"
      :title="$t('asset-detail-kind-change-title')"
      :message="kindChangeConfirmMessage"
      :confirm-label="$t('asset-detail-kind-change-confirm-label')"
      @confirm="confirmKindChange"
      @close="cancelKindChange"
    />
  </div>
</template>
