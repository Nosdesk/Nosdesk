<script setup lang="ts">
import { ref, computed, watch, onBeforeUnmount } from 'vue';
import { useRoute, useRouter, RouterLink } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useTitleManager } from '@/composables/useTitleManager';
import { formatDateTime } from '@nosdesk/core/utils/dateUtils';
import BackButton from '@/components/common/BackButton.vue';
import SearchableDropdown, { type DropdownOption } from '@/components/common/SearchableDropdown.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Button from '@/components/common/Button.vue';
import DeleteButton from '@/components/common/DeleteButton.vue';
import FormInput from '@/components/common/FormInput.vue';
import InlineEdit from '@/components/common/InlineEdit.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import DatePicker from '@/components/common/DatePicker.vue';
import { extractErrorMessage } from '@/utils/errors';
import Icon from '@/components/common/Icon.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import UserSelectionModal from '@/components/UserSelectionModal.vue';
import DeviceGroups from '@/components/AssetGroups.vue';
import AssetGroupEditor from '@/components/assets/AssetGroupEditor.vue';
import AssetMediaPanel from '@/components/assets/AssetMediaPanel.vue';
import AssetLifecyclePanel from '@/components/assets/AssetLifecyclePanel.vue';
import AssetLoanPanel from '@/components/assets/AssetLoanPanel.vue';
import AssetUsageHistory from '@/components/assets/AssetUsageHistory.vue';
import AssetModelField from '@/components/assets/AssetModelField.vue';
import { kindIconName } from '@/components/assets/assetKindIcon';
import {
  SYNC_OWNED_ATTRIBUTE_KEYS,
  userAttributeSchema as buildUserAttributeSchema,
  syncAttributeSchema as buildSyncAttributeSchema,
} from '@/components/assets/assetAttributeSchema';
import PluginSlot from '@/plugins/components/PluginSlot.vue';
import Modal from '@/components/Modal.vue';
import { deleteAsset, downloadAssetRecordCard, unmanageAsset } from '@/services/assetService';
import { type AssetKind } from '@nosdesk/core/services/assetKindsService';
import { useAssetKindsQuery } from '@/composables/useAssetKindsQuery';
import { useAssetLocationsQuery } from '@/composables/useAssetLocationsQuery';
import { useSyncActions } from '@/composables/useSyncActions';
import { useAssetDetail } from '@/composables/useAssetDetail';
import { useAuthStore } from '@/stores/auth';
import type { Asset } from '@nosdesk/core/types/asset';
import DynamicAttributeForm from '@/components/assets/DynamicAttributeForm.vue';

const route = useRoute();
const router = useRouter();
const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const emit = defineEmits(['update:device']);

// State. The asset record and its persistence live in a single Pinia Colada
// cache (useAssetDetail): `device` is the read-only cached source of truth and
// `patchAsset` is the only writer (optimistic + rollback + invalidate). There
// is no local copy of the record. The old dual source of truth (a `device` ref
// plus `attributeDraft`/`editValues` drafts that every refetch reset) was racy:
// a debounced PUT could be clobbered by a concurrent refetch and silently
// no-op. One source of truth removes the race and surfaces save failures.
const assetId = computed(() => Number(route.params.id));
const {
  asset: device,
  isFirstLoad: loading,
  error: loadError,
  invalidate: invalidateAsset,
  setAsset,
  patchAsset,
} = useAssetDetail(assetId);

const mediaPanelRef = ref<InstanceType<typeof AssetMediaPanel> | null>(null);
const error = ref<string | null>(null);
const isSaving = ref(false);
const showUserSelectionModal = ref(false);
const showManagedByModal = ref(false);
const showUnmanageModal = ref(false);
const unmanageError = ref<string | null>(null);
const { locations: knownLocations } = useAssetLocationsQuery();

// Edit buffer for the inline column fields, seeded from the cached record by a
// watch (below). InlineEdit/DatePicker guard their own buffers against mid-edit
// re-sync, and every commit persists through `patchAsset`.
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

// Asset-kind registry (technician-gated list, shared cache).
const { kinds: kindsRef } = useAssetKindsQuery();
const kinds = computed<AssetKind[]>(() => kindsRef.value);
const selectedKindSlug = ref<string>('generic');

const selectedKind = computed<AssetKind | null>(
  () => kinds.value.find((k) => k.slug === selectedKindSlug.value) ?? null,
);
const selectedKindCategory = computed(() => selectedKind.value?.category ?? 'generic');

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

const isEditable = computed(() => device.value?.is_editable ?? false);
const isSynced = computed(() => device.value != null && !device.value.is_editable);

// The user-editable and sync-owned slices of the kind's schema come
// from the shared partition util. Sync-owned keys (Intune / Entra) are
// written by the sync, never typed by a human; they render read-only in
// a "Synced from …" panel, and only when the asset is actually
// sync-owned. Everything else is a user-editable field.
const userAttributeSchema = computed(() =>
  buildUserAttributeSchema(selectedKindSchema.value),
);
const syncAttributeSchema = computed(() =>
  buildSyncAttributeSchema(selectedKindSchema.value),
);

// Surface the sync panel when the asset is sync-owned, or defensively
// when a sync attribute already carries a value.
const hasSyncData = computed(() => {
  const attrs = device.value?.attributes ?? {};
  return Object.keys(attrs).some(
    (k) => SYNC_OWNED_ATTRIBUTE_KEYS.has(k) && attrs[k] != null && attrs[k] !== '',
  );
});
const showSyncPanel = computed(() => syncAttributeSchema.value != null && (isSynced.value || hasSyncData.value));

const syncSourceLabel = computed(() => {
  const src = device.value?.external_sync_source;
  if (src === 'intune') return t('asset-detail-sync-source-intune');
  if (src === 'entra') return t('asset-detail-sync-source-entra');
  return t('asset-detail-sync-source-generic');
});

// ---- Extend-on-demand properties --------------------------------
//
// Optional properties follow the ticket-detail model: a property is
// shown when it already carries a value, when its kind expects it by
// default (an owner for IT/logical, stock for bulk), or when the user
// adds it from the "Add property" menu. A brand-new asset shows only
// Type, so the surface stays straightforward until the user extends it.
type PropKey =
  | 'primary_user'
  | 'managed_by'
  | 'serial_number'
  | 'manufacturer'
  | 'model'
  | 'asset_tag'
  | 'location'
  | 'purchase_date'
  | 'stock';

const PROP_LABEL_KEY: Record<PropKey, string> = {
  primary_user: 'asset-detail-section-primary-user',
  managed_by: 'asset-detail-section-managed-by',
  serial_number: 'asset-detail-field-serial',
  manufacturer: 'asset-detail-field-manufacturer',
  model: 'asset-detail-field-model',
  asset_tag: 'asset-detail-field-asset-tag',
  location: 'asset-detail-field-location',
  purchase_date: 'asset-detail-field-purchase-date',
  stock: 'asset-detail-section-stock',
};

const PROP_ORDER: PropKey[] = [
  'primary_user',
  'managed_by',
  'serial_number',
  'manufacturer',
  'model',
  'asset_tag',
  'location',
  'purchase_date',
  'stock',
];

// Which universal properties are relevant per category. Drives the
// "Add property" menu so a generic asset isn't offered hardware fields
// (serial / make / model) and a consumable isn't offered an owner. A
// property that already holds a value always shows regardless of this,
// so changing an asset's type never hides existing data.
const RELEVANT_PROPS: Record<string, PropKey[]> = {
  it: ['primary_user', 'managed_by', 'serial_number', 'manufacturer', 'model', 'asset_tag', 'location', 'purchase_date'],
  physical: ['primary_user', 'managed_by', 'serial_number', 'manufacturer', 'model', 'asset_tag', 'location', 'purchase_date'],
  logical: ['primary_user', 'managed_by', 'manufacturer', 'asset_tag', 'purchase_date'],
  bulk: ['stock', 'manufacturer', 'asset_tag', 'location', 'purchase_date'],
  generic: ['primary_user', 'managed_by', 'location', 'asset_tag', 'purchase_date'],
};

const relevantSet = computed(
  () => new Set<PropKey>(RELEVANT_PROPS[selectedKindCategory.value] ?? RELEVANT_PROPS.generic),
);

// Properties the chosen kind surfaces by default even when empty.
function isDefaultProp(key: PropKey): boolean {
  const cat = selectedKindCategory.value;
  if (key === 'primary_user') return cat === 'it' || cat === 'logical';
  if (key === 'stock') return cat === 'bulk';
  return false;
}

function propHasValue(key: PropKey): boolean {
  const d = device.value;
  if (!d) return false;
  switch (key) {
    case 'primary_user':
      return Boolean(d.primary_user_uuid);
    case 'managed_by':
      return Boolean(d.managed_by_user_uuid);
    case 'serial_number':
      return Boolean(d.serial_number);
    case 'manufacturer':
      return Boolean(d.manufacturer);
    case 'model':
      return Boolean(d.model);
    case 'asset_tag':
      return Boolean(d.asset_tag);
    case 'location':
      return Boolean(d.location);
    case 'purchase_date':
      return Boolean(d.purchase_date);
    case 'stock':
      return d.quantity != null || Boolean(d.unit) || d.low_stock_threshold != null;
  }
}

// Properties the user explicitly added this session.
const revealed = ref<Set<PropKey>>(new Set());

function isPropVisible(key: PropKey): boolean {
  return propHasValue(key) || isDefaultProp(key) || revealed.value.has(key);
}

const visibleProps = computed(() => PROP_ORDER.filter((k) => isPropVisible(k)));
const addableProps = computed(() =>
  PROP_ORDER.filter((k) => relevantSet.value.has(k) && !isPropVisible(k)),
);

// User-owned kind attributes participate in the same extend-on-demand
// list as the universal columns: each schema property is its own
// addable property, rendered by a single-field DynamicAttributeForm.
const userAttrKeys = computed<string[]>(() =>
  Object.keys((userAttributeSchema.value?.properties as Record<string, unknown>) ?? {}),
);
function attrHasValue(key: string): boolean {
  const v = device.value?.attributes?.[key];
  return v != null && v !== '';
}
const revealedAttrs = ref<Set<string>>(new Set());
function isAttrVisible(key: string): boolean {
  return attrHasValue(key) || revealedAttrs.value.has(key);
}
const visibleAttrKeys = computed(() => userAttrKeys.value.filter(isAttrVisible));
const addableAttrKeys = computed(() => userAttrKeys.value.filter((k) => !isAttrVisible(k)));
function attrTitle(key: string): string {
  const props = (userAttributeSchema.value?.properties as Record<string, { title?: string }>) ?? {};
  return props[key]?.title ?? key;
}
function singleAttrSchema(key: string): Record<string, unknown> | null {
  const props = (userAttributeSchema.value?.properties as Record<string, unknown>) ?? {};
  if (!(key in props)) return null;
  return { type: 'object', properties: { [key]: props[key] } };
}

// Attribute clusters that read as one concept: the menu offers them as
// a single entry that reveals every field at once (e.g. warranty status
// + start + end), instead of three separate picks.
const ATTR_GROUPS: Record<string, string[]> = {
  warranty: ['warranty_status', 'warranty_start_date', 'warranty_end_date'],
};
const groupedAttrKeys = new Set(Object.values(ATTR_GROUPS).flat());
/** A group's keys that actually exist in the current kind's schema. */
function groupKeysInSchema(group: string): string[] {
  return (ATTR_GROUPS[group] ?? []).filter((k) => userAttrKeys.value.includes(k));
}

// The Add-property menu offers universal columns, grouped attribute
// clusters, then any remaining individual attributes; values are
// namespaced so the handler knows which.
const addPropOptions = computed<DropdownOption[]>(() => [
  // Columns are only addable on manual assets (synced columns are
  // locked); user attributes are addable either way.
  ...(isEditable.value
    ? addableProps.value.map((k) => ({ value: `col:${k}`, label: t(PROP_LABEL_KEY[k]) }))
    : []),
  // A group shows while any of its fields is still hidden.
  ...Object.keys(ATTR_GROUPS)
    .filter((g) => groupKeysInSchema(g).some((k) => !isAttrVisible(k)))
    .map((g) => ({ value: `group:${g}`, label: t(`asset-detail-group-${g}`) })),
  ...addableAttrKeys.value
    .filter((k) => !groupedAttrKeys.has(k))
    .map((k) => ({ value: `attr:${k}`, label: attrTitle(k) })),
]);

const addPropModel = ref('');
function onAddProp(value: string) {
  if (!value) return;
  if (value.startsWith('group:')) {
    const next = new Set(revealedAttrs.value);
    groupKeysInSchema(value.slice(6)).forEach((k) => next.add(k));
    revealedAttrs.value = next;
  } else if (value.startsWith('attr:')) {
    revealedAttrs.value = new Set(revealedAttrs.value).add(value.slice(5));
  } else {
    const key = value.startsWith('col:') ? value.slice(4) : value;
    revealed.value = new Set(revealed.value).add(key as PropKey);
  }
  addPropModel.value = '';
}

// User-owned attributes (warranty etc.) commit eagerly: each change persists
// immediately through the optimistic mutation, with no debounce window to lose
// and no shared draft for a concurrent refetch to clobber. DynamicAttributeForm
// emits the full attribute object with the edited key applied.
const attributesError = ref<string | null>(null);
async function commitAttribute(next: Record<string, unknown>) {
  if (!device.value) return;
  attributesError.value = null;
  try {
    await patchAsset({ attributes: next });
  } catch (err) {
    attributesError.value = extractErrorMessage(err, t('asset-detail-attributes-save-failed'));
  }
}

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

const managementLabel = computed(() =>
  isSynced.value
    ? t('asset-detail-external-sync-source', { source: device.value?.external_sync_source || '' })
    : t('asset-detail-manually-managed'),
);

const isKindEditable = computed(() => isEditable.value);

const kindChangeError = ref<string | null>(null);

/**
 * Change the asset's kind inline, non-destructively. The category
 * swap changes which properties the kind surfaces by default, so we
 * first pin everything currently visible into `revealed` to stop rows
 * from collapsing out from under the user. Custom attributes are
 * pruned to the keys the new schema accepts (the backend validator
 * rejects unknown keys) rather than wiped, so compatible values carry
 * across. No confirm dialog: this behaves like editing any property.
 */
/** Re-hydrate after the model picker stamps/clears a model: the backend returns
 *  the updated asset (manufacturer/model/kind/attributes applied). Push it into
 *  the cache as the authoritative record; the watch below reseeds the buffer. */
function onAssetModelUpdated(asset: Asset) {
  setAsset(asset);
}

async function changeKind(newSlug: string) {
  if (!device.value) return;
  const currentSlug = device.value.kind ?? 'generic';
  if (newSlug === currentSlug) return;

  // Keep the visible property set stable across the category change.
  revealed.value = new Set<PropKey>([...revealed.value, ...visibleProps.value]);
  selectedKindSlug.value = newSlug;

  const newKind = kinds.value.find((k) => k.slug === newSlug);
  const allowed = new Set(
    Object.keys((newKind?.attribute_schema?.properties as Record<string, unknown>) ?? {}),
  );
  const prunedAttributes: Record<string, unknown> = {};
  for (const [key, val] of Object.entries(device.value?.attributes ?? {})) {
    if (allowed.has(key)) prunedAttributes[key] = val;
  }

  isSaving.value = true;
  kindChangeError.value = null;
  try {
    await patchAsset({ kind: newSlug, attributes: prunedAttributes });
  } catch (err) {
    kindChangeError.value = extractErrorMessage(err, t('asset-detail-kind-change-failed'));
    selectedKindSlug.value = device.value?.kind ?? 'generic';
  } finally {
    isSaving.value = false;
  }
}

// Seed the inline-field edit buffer + kind selector from the cached record.
// Runs on initial load and whenever the authoritative record changes (a save's
// reconciliation, a model stamp, or an external SSE update). InlineEdit /
// DatePicker guard their own buffers, so a reseed never interrupts an edit.
function seedFromAsset(a: Asset) {
  editValues.value = {
    name: a.name,
    manufacturer: a.manufacturer || '',
    model: a.model,
    serial_number: a.serial_number,
    location: a.location || '',
    purchase_date: a.purchase_date || '',
    asset_tag: a.asset_tag || '',
    quantity: a.quantity ?? '',
    unit: a.unit ?? '',
    low_stock_threshold: a.low_stock_threshold ?? '',
  };
  selectedKindSlug.value = a.kind ?? 'generic';
}

async function selectLocationSuggestion(location: string) {
  editValues.value.location = location;
  await saveField('location');
}

const saveField = async (field: keyof typeof editValues.value) => {
  if (!device.value) return;
  try {
    isSaving.value = true;
    await patchAsset({ [field]: editValues.value[field] });
  } catch (err) {
    console.error('Error saving device field:', err);
    // patchAsset rolled the cache back; re-sync the buffer to the stored value.
    if (device.value) {
      editValues.value[field] = (device.value[field as keyof Asset] as string) || '';
    }
  } finally {
    isSaving.value = false;
  }
};

/** Inline-edit a stock-tracking field. Empty input is omitted from
 *  the PATCH body so the backend leaves the column untouched. */
const saveStockField = async (field: 'quantity' | 'unit' | 'low_stock_threshold') => {
  if (!device.value) return;
  const raw = editValues.value[field].trim();
  if (raw === '') {
    editValues.value[field] = (device.value[field] as string | null | undefined) ?? '';
    return;
  }
  try {
    isSaving.value = true;
    await patchAsset({ [field]: raw });
  } catch (err) {
    console.error('Error saving stock field:', err);
    editValues.value[field] = (device.value?.[field] as string | null | undefined) ?? '';
  } finally {
    isSaving.value = false;
  }
};

const isLowStock = computed(() => {
  const q = device.value?.quantity;
  const threshold = device.value?.low_stock_threshold;
  if (q == null || threshold == null) return false;
  if (q === '' || threshold === '') return false;
  return parseFloat(q) <= parseFloat(threshold);
});

const handleUserSelection = async (user: { uuid: string; name: string; email: string; role: string }) => {
  if (!device.value) return;
  showUserSelectionModal.value = false;
  try {
    isSaving.value = true;
    await patchAsset({ primary_user_uuid: user.uuid || null });
  } catch (err) {
    console.error('Error updating device user:', err);
  } finally {
    isSaving.value = false;
  }
};

async function clearPrimaryUser() {
  if (!device.value) return;
  try {
    isSaving.value = true;
    await patchAsset({ primary_user_uuid: null });
  } catch (err) {
    console.error('Error clearing primary user:', err);
  } finally {
    isSaving.value = false;
  }
}

const handleManagedBySelection = async (user: { uuid: string; name: string; email: string; role: string }) => {
  if (!device.value) return;
  showManagedByModal.value = false;
  try {
    isSaving.value = true;
    await patchAsset({ managed_by_user_uuid: user.uuid || null });
  } catch (err) {
    console.error('Error updating managed-by user:', err);
  } finally {
    isSaving.value = false;
  }
};

async function clearManagedBy() {
  if (!device.value) return;
  try {
    isSaving.value = true;
    await patchAsset({ managed_by_user_uuid: null });
  } catch (err) {
    console.error('Error clearing managed-by user:', err);
  } finally {
    isSaving.value = false;
  }
}

async function downloadRecordCard() {
  if (!device.value) return;
  try {
    await downloadAssetRecordCard(device.value.id);
  } catch (err) {
    console.error('Error downloading record card:', err);
  }
}

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
    setAsset(updatedDevice);
    showUnmanageModal.value = false;
  } catch (err) {
    console.error('Error unmanaging device:', err);
    unmanageError.value = t('asset-detail-error-unmanage');
  } finally {
    isSaving.value = false;
  }
};

// Reseed the inline-field buffer whenever the authoritative record changes
// (initial load, a save's reconciliation, a model stamp, or an external update).
watch(device, (d) => { if (d) seedFromAsset(d); }, { immediate: true });

// Propagate the full record to the layout (drives the header title).
watch(device, (newDevice) => {
  if (newDevice) emit('update:device', newDevice);
}, { immediate: true, deep: true });

// Surface a load failure (the query owns the fetch).
watch(loadError, (e) => {
  if (e) error.value = t('asset-detail-error-load');
});

// The asset name lives in the site header (like a ticket title). Register
// the save handler only while the asset is editable, so a sync-owned
// asset shows its name read-only.
const titleManager = useTitleManager();
const saveDeviceName = async (newName: string) => {
  if (!device.value) return;
  editValues.value.name = newName;
  await saveField('name');
};
watch(isEditable, (editable) => {
  titleManager.onDeviceTitleSave(editable ? saveDeviceName : null);
}, { immediate: true });
onBeforeUnmount(() => {
  titleManager.onDeviceTitleSave(null);
  titleManager.clearDevice();
});

// The query keys on `assetId`, so navigating to a different asset refetches
// automatically; just reset the per-session reveal state.
watch(() => route.params.id, () => {
  revealed.value = new Set();
  revealedAttrs.value = new Set();
});

const auth = useAuthStore();
const canChangeLifecycle = computed(() => auth.isTechnician);

// A status transition from the lifecycle panel doesn't flow back through
// the own-change sync filter, so reflect it optimistically (the status
// badge updates at once) and refetch for the authoritative record.
function onAssetTransitioned(toStatus: string) {
  if (device.value) setAsset({ ...device.value, status: toStatus });
  void invalidateAsset();
}
useSyncActions(
  (actions) => {
    const id = device.value?.id;
    if (id == null) return;
    const mine = auth.user?.uuid ?? null;
    const relevant = actions.filter((a) => a.aggregate_id === String(id) && a.actor_uuid !== mine);
    if (relevant.length === 0) return;
    if (relevant.some((a) => a.op === 'D')) {
      router.push('/assets');
      return;
    }
    void invalidateAsset();
  },
  { aggregates: ['asset'], debounceMs: 300 },
);
</script>

<template>
  <div class="flex-1">
    <!-- Loading -->
    <div v-if="loading" class="flex justify-center items-center min-h-[200px]">
      <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-accent"></div>
    </div>

    <!-- Main content -->
    <div v-else-if="device" class="flex flex-col">
      <!-- Top bar. Shares the content's max width so the back button
           lines up with the body on wide screens. -->
      <div class="pt-4 px-4 sm:px-6 mx-auto w-full max-w-8xl flex items-center justify-between gap-3">
        <BackButton fallbackRoute="/assets" :label="$t('asset-detail-back-to-devices')" compact />
        <div class="flex items-center gap-2">
          <DeleteButton
            v-if="isEditable"
            fallbackRoute="/assets"
            :itemName="$t('asset-detail-delete-item-name')"
            @delete="handleDeleteDevice"
          />
        </div>
      </div>

      <div class="flex flex-col gap-4 px-4 py-4 sm:px-6 mx-auto w-full max-w-8xl">
        <AlertMessage v-if="error" type="error" :message="error" />

        <!-- Sync context. The name lives in the site header (like a
             ticket title) and status is shown in the lifecycle panel. -->
        <div v-if="isSynced" class="flex flex-wrap items-center gap-2">
          <span
            class="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs font-medium bg-surface-alt text-secondary border border-default"
          >
            <Icon name="lock" size="xs" />
            {{ $t('asset-detail-readonly') }}
          </span>
        </div>

        <div class="asset-grid items-start">
          <!-- Primary column: the property list (extend on demand) and
               the sub-record panels. Properties are the heart of the
               asset, so they get the wide column, not a thin rail. -->
          <div class="asset-main">
            <SectionCard content-padding="p-4">
              <template #title>{{ $t('asset-detail-section-details') }}</template>

              <div class="grid grid-cols-1 sm:grid-cols-2 gap-x-5 gap-y-3">
                <!-- Model. The primary "what is this?" control: pick a
                     real make+model from the catalog and the asset is
                     stamped (manufacturer, model, kind, specs). -->
                <div class="sm:col-span-2">
                  <AssetModelField
                    :asset-id="device.id"
                    :model-id="device.model_id ?? null"
                    :kind="selectedKindSlug"
                    :manufacturer-snapshot="device.manufacturer ?? null"
                    :model-snapshot="device.model || null"
                    :editable="isEditable"
                    @updated="onAssetModelUpdated"
                  />
                </div>

                <!-- Type. Owned by the model when one is linked; editable
                     directly only for model-less assets. -->
                <div class="flex flex-col gap-1 sm:col-span-2">
                  <h3 class="text-xs font-medium text-tertiary">{{ $t('asset-detail-field-kind') }}</h3>
                  <div class="flex items-center gap-2">
                    <Icon
                      :name="kindIconName(selectedKind)"
                      size="sm"
                      class="text-secondary flex-shrink-0"
                    />
                    <SearchableDropdown
                      v-if="isKindEditable && kinds.length > 0 && !device.model_id"
                      class="flex-1 min-w-0"
                      :model-value="selectedKindSlug"
                      :options="kindOptions"
                      size="sm"
                      @update:model-value="(value) => changeKind(String(value))"
                    />
                    <p v-else class="text-sm text-primary">{{ selectedKind?.label ?? selectedKindSlug }}</p>
                  </div>
                  <AlertMessage v-if="kindChangeError" type="error" :message="kindChangeError" />
                </div>

                <!-- Revealed optional properties, in canonical order. -->
                <template v-for="key in visibleProps" :key="key">
                  <!-- Primary user -->
                  <div v-if="key === 'primary_user'" class="flex flex-col gap-1 sm:col-span-2">
                    <div class="flex items-center justify-between min-h-6">
                      <h3 class="text-xs font-medium text-tertiary">{{ $t('asset-detail-section-primary-user') }}</h3>
                      <button
                        v-if="device.primary_user && isEditable"
                        type="button"
                        class="p-1 text-tertiary hover:text-status-error hover:bg-status-error-muted rounded transition-colors"
                        :title="$t('asset-detail-clear-user')"
                        @click="clearPrimaryUser"
                      >
                        <Icon name="close" />
                      </button>
                    </div>
                    <RouterLink
                    v-if="device.primary_user"
                    :to="`/users/${device.primary_user.uuid}`"
                    class="group flex items-center gap-2.5 min-w-0"
                  >
                    <UserAvatar
                      :uuid="device.primary_user.uuid"
                      :fallback-name="device.primary_user.name"
                      :fallback-avatar="device.primary_user.avatar_thumb || device.primary_user.avatar_url"
                      size="sm"
                      :clickable="false"
                      :show-name="false"
                    />
                    <div class="min-w-0">
                      <span class="block text-sm font-medium text-primary truncate group-hover:text-accent transition-colors">{{ device.primary_user.name }}</span>
                      <span class="block text-xs text-tertiary truncate">{{ device.primary_user.email }}</span>
                    </div>
                  </RouterLink>
                    <Button
                      v-else-if="isEditable"
                      block
                      variant="secondary"
                      size="sm"
                      icon="user"
                      @click="showUserSelectionModal = true"
                    >
                      {{ $t('asset-detail-action-assign-user') }}
                    </Button>
                  </div>

                  <!-- Managed by: accountable custodian, distinct from the primary user (holder). -->
                  <div v-else-if="key === 'managed_by'" class="flex flex-col gap-1 sm:col-span-2">
                    <div class="flex items-center justify-between min-h-6">
                      <h3 class="text-xs font-medium text-tertiary">{{ $t('asset-detail-section-managed-by') }}</h3>
                      <button
                        v-if="device.managed_by_user_uuid && isEditable"
                        type="button"
                        class="p-1 text-tertiary hover:text-status-error hover:bg-status-error-muted rounded transition-colors"
                        :title="$t('asset-detail-clear-managed-by')"
                        @click="clearManagedBy"
                      >
                        <Icon name="close" />
                      </button>
                    </div>
                    <RouterLink
                      v-if="device.managed_by_user_uuid"
                      :to="`/users/${device.managed_by_user_uuid}`"
                      class="group flex items-center gap-2.5 min-w-0"
                    >
                      <UserAvatar
                        :uuid="device.managed_by_user_uuid"
                        size="sm"
                        :clickable="false"
                        show-name
                      />
                    </RouterLink>
                    <Button
                      v-else-if="isEditable"
                      block
                      variant="secondary"
                      size="sm"
                      icon="user"
                      @click="showManagedByModal = true"
                    >
                      {{ $t('asset-detail-action-assign-managed-by') }}
                    </Button>
                  </div>

                  <!-- Serial -->
                  <div v-else-if="key === 'serial_number'" class="flex flex-col gap-1">
                    <h3 class="text-xs font-medium text-tertiary">{{ $t('asset-detail-field-serial') }}</h3>
                    <InlineEdit
                      v-model="editValues.serial_number"
                      :placeholder="$t('asset-detail-field-serial-placeholder-edit')"
                      text-size="sm"
                      :can-edit="isEditable"
                      @update:modelValue="() => saveField('serial_number')"
                    />
                  </div>

                  <!-- Manufacturer -->
                  <div v-else-if="key === 'manufacturer'" class="flex flex-col gap-1">
                    <h3 class="text-xs font-medium text-tertiary">{{ $t('asset-detail-field-manufacturer') }}</h3>
                    <InlineEdit
                      v-model="editValues.manufacturer"
                      :placeholder="$t('asset-detail-field-manufacturer-placeholder-edit')"
                      text-size="sm"
                      :can-edit="isEditable"
                      @update:modelValue="() => saveField('manufacturer')"
                    />
                  </div>

                  <!-- Model -->
                  <div v-else-if="key === 'model'" class="flex flex-col gap-1">
                    <h3 class="text-xs font-medium text-tertiary">{{ $t('asset-detail-field-model') }}</h3>
                    <InlineEdit
                      v-model="editValues.model"
                      :placeholder="$t('asset-detail-field-model-placeholder-edit')"
                      text-size="sm"
                      :can-edit="isEditable"
                      @update:modelValue="() => saveField('model')"
                    />
                  </div>

                  <!-- Asset tag -->
                  <div v-else-if="key === 'asset_tag'" class="flex flex-col gap-1">
                    <h3 class="text-xs font-medium text-tertiary">{{ $t('asset-detail-field-asset-tag') }}</h3>
                    <InlineEdit
                      v-model="editValues.asset_tag"
                      :placeholder="$t('asset-detail-field-asset-tag-placeholder-edit')"
                      text-size="sm"
                      :can-edit="isEditable"
                      @update:modelValue="() => saveField('asset_tag')"
                    />
                  </div>

                  <!-- Location -->
                  <div v-else-if="key === 'location'" class="flex flex-col gap-1 sm:col-span-2">
                    <h3 class="text-xs font-medium text-tertiary">{{ $t('asset-detail-field-location') }}</h3>
                    <InlineEdit
                      v-model="editValues.location"
                      :placeholder="$t('asset-detail-field-location-placeholder-edit')"
                      text-size="sm"
                      :can-edit="isEditable"
                      @update:modelValue="() => saveField('location')"
                    />
                    <div
                      v-if="isEditable && locationSuggestions.length"
                      class="flex flex-wrap items-center gap-1.5"
                    >
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

                  <!-- Purchase date -->
                  <div v-else-if="key === 'purchase_date'" class="flex flex-col gap-1">
                    <h3 class="text-xs font-medium text-tertiary">{{ $t('asset-detail-field-purchase-date') }}</h3>
                    <DatePicker
                      v-if="isEditable"
                      v-model="editValues.purchase_date"
                      size="sm"
                      block
                      :aria-label="$t('asset-detail-field-purchase-date')"
                      @update:model-value="() => saveField('purchase_date')"
                    />
                    <p v-else class="text-primary text-sm">{{ device.purchase_date || '-' }}</p>
                  </div>

                  <!-- Stock tracking -->
                  <div v-else-if="key === 'stock'" class="flex flex-col gap-2 sm:col-span-2">
                    <h3 class="text-xs font-medium text-tertiary">{{ $t('asset-detail-section-stock') }}</h3>
                    <div
                      v-if="isLowStock"
                      class="flex items-center gap-2 px-3 py-2 bg-status-warning/10 text-status-warning rounded-lg text-sm"
                    >
                      <Icon name="warning" class="flex-shrink-0" />
                      <span>{{ $t('asset-detail-low-stock-warning', { quantity: device.quantity ?? '', unit: device.unit ?? '', threshold: device.low_stock_threshold ?? '' }) }}</span>
                    </div>
                    <div class="grid grid-cols-2 gap-2">
                      <FormInput
                        v-model="editValues.quantity"
                        :label="$t('asset-detail-field-quantity')"
                        :placeholder="$t('asset-detail-field-quantity-placeholder')"
                        inputmode="decimal"
                        size="sm"
                        :disabled="!isEditable"
                        @blur="saveStockField('quantity')"
                        @keyup.enter="saveStockField('quantity')"
                      />
                      <FormInput
                        v-model="editValues.unit"
                        :label="$t('asset-detail-field-unit')"
                        :placeholder="$t('asset-detail-field-unit-placeholder')"
                        size="sm"
                        :disabled="!isEditable"
                        @blur="saveStockField('unit')"
                        @keyup.enter="saveStockField('unit')"
                      />
                      <FormInput
                        v-model="editValues.low_stock_threshold"
                        class="col-span-2"
                        :label="$t('asset-detail-field-low-stock-threshold')"
                        :placeholder="$t('asset-detail-field-low-stock-threshold-placeholder')"
                        inputmode="decimal"
                        size="sm"
                        :disabled="!isEditable"
                        @blur="saveStockField('low_stock_threshold')"
                        @keyup.enter="saveStockField('low_stock_threshold')"
                      />
                    </div>
                  </div>
                </template>

                <!-- User-owned kind attributes (e.g. warranty), each its
                     own extend-on-demand field. Editable even on synced
                     assets: the backend only lets these keys change, with
                     sync-owned keys staying locked. Autosaves on input. -->
                <div
                  v-for="key in visibleAttrKeys"
                  :key="`attr-${key}`"
                  class="sm:col-span-2"
                >
                  <DynamicAttributeForm
                    :schema="singleAttrSchema(key)"
                    :model-value="device?.attributes ?? {}"
                    @update:model-value="commitAttribute"
                  />
                </div>
                <AlertMessage
                  v-if="attributesError"
                  type="error"
                  :message="attributesError"
                  class="sm:col-span-2"
                />

                <!-- Add property -->
                <div v-if="addPropOptions.length > 0" class="sm:col-span-2">
                  <BaseDropdown
                    :model-value="addPropModel"
                    :options="addPropOptions"
                    :placeholder="$t('asset-detail-add-property')"
                    size="sm"
                    @update:model-value="(v) => onAddProp(v as string)"
                  />
                </div>
              </div>
            </SectionCard>

            <!-- Sync-owned device telemetry (Intune / Entra). Read-only
                 and shown only when the asset is sync-owned: this data
                 comes from the Microsoft Graph sync, not manual entry. -->
            <SectionCard v-if="showSyncPanel" content-padding="p-4">
              <template #leading>
                <Icon name="refresh" size="sm" class="text-secondary" />
              </template>
              <template #title>{{ syncSourceLabel }}</template>
              <DynamicAttributeForm
                :schema="syncAttributeSchema"
                :model-value="device?.attributes ?? {}"
                :disabled="true"
              />
            </SectionCard>

            <!-- Sub-record panels: lifecycle, loans, usage, media,
                 plugins. These accrue over an asset's life rather than
                 being creation inputs, so they sit below the property
                 list in the same wide column. -->
            <SectionCard content-padding="p-4">
              <template #title>{{ $t('asset-lifecycle-heading') }}</template>
              <template #headerActions>
                <button
                  type="button"
                  class="p-1 -mr-1 text-tertiary hover:text-primary hover:bg-surface-hover rounded transition-colors"
                  :title="$t('asset-detail-record-card')"
                  :aria-label="$t('asset-detail-record-card')"
                  @click="downloadRecordCard"
                >
                  <Icon name="download" size="sm" />
                </button>
              </template>
              <AssetLifecyclePanel
                :asset-id="device.id"
                :current-status="device.status"
                :can-edit="canChangeLifecycle"
                @transitioned="onAssetTransitioned"
              />
            </SectionCard>

            <SectionCard content-padding="p-4">
              <template #title>{{ $t('asset-loan-heading') }}</template>
              <AssetLoanPanel
                :asset-id="device.id"
                :current-status="device.status"
                :can-edit="canChangeLifecycle"
                @changed="() => invalidateAsset()"
              />
            </SectionCard>

            <SectionCard v-if="device.quantity != null" content-padding="p-4">
              <template #title>{{ $t('asset-usage-history-heading') }}</template>
              <AssetUsageHistory
                :asset-id="device.id"
                :unit="device.unit"
                :current-quantity="device.quantity"
                @recorded="invalidateAsset"
              />
            </SectionCard>

            <PluginSlot slot-name="asset-info-panels" :device="device" />
          </div>

          <!-- Rail: groups + record metadata (small, secondary facts). -->
          <aside class="asset-rail">
            <AssetGroupEditor
              :asset-id="device.id"
              :groups="device.asset_groups ?? []"
              :editable="isEditable"
              @update:groups="(groups) => { if (device) device.asset_groups = groups }"
            />

            <!-- Directory (Intune/Entra) memberships, read-only. -->
            <DeviceGroups v-if="device.groups?.length" :groups="device.groups" />

            <SectionCard content-padding="p-4">
              <template #title>{{ $t('asset-detail-section-record') }}</template>
              <div class="flex flex-col gap-3 text-sm">
                <div class="flex items-center justify-between gap-3">
                  <span class="text-xs font-medium uppercase tracking-wide text-tertiary">{{ $t('asset-detail-field-asset-id') }}</span>
                  <span class="font-mono text-primary">{{ device.id }}</span>
                </div>
                <div class="flex items-center justify-between gap-3">
                  <span class="text-xs font-medium uppercase tracking-wide text-tertiary">{{ $t('asset-detail-field-created') }}</span>
                  <span class="text-primary text-right">{{ formatDateTime(device.created_at) }}</span>
                </div>
                <div class="flex items-center justify-between gap-3">
                  <span class="text-xs font-medium uppercase tracking-wide text-tertiary">{{ $t('asset-detail-field-last-updated') }}</span>
                  <span class="text-primary text-right">{{ formatDateTime(device.updated_at) }}</span>
                </div>
                <div class="pt-3 border-t border-default flex items-start gap-2">
                  <Icon :name="isSynced ? 'refresh' : 'info'" size="sm" class="text-secondary flex-shrink-0 mt-0.5" />
                  <div class="min-w-0">
                    <p class="font-medium text-primary">{{ managementLabel }}</p>
                    <p class="text-xs text-tertiary mt-0.5">
                      {{ isSynced ? $t('asset-detail-external-sync-note') : $t('asset-detail-manually-managed-description') }}
                    </p>
                  </div>
                </div>
                <template v-if="isSynced">
                  <Button
                    @click="handleUnmanageDevice"
                    :disabled="isSaving"
                    block
                    variant="secondary"
                    size="sm"
                    icon="refresh"
                    :title="$t('asset-detail-action-unmanage-title')"
                  >
                    {{ isSaving ? $t('asset-detail-action-unmanage-processing') : $t('asset-detail-action-unmanage') }}
                  </Button>
                  <p class="text-xs text-tertiary">{{ $t('asset-detail-unmanage-conversion-note') }}</p>
                </template>
              </div>
            </SectionCard>

            <!-- Photos: a compact gallery in the rail. Reference imagery
                 belongs alongside the record metadata, not in the wide
                 primary column. The add control sits in the card header. -->
            <SectionCard content-padding="p-4">
              <template #title>{{ $t('asset-media-heading') }}</template>
              <template #headerActions>
                <button
                  v-if="device.is_editable"
                  type="button"
                  class="p-1 -mr-1 text-tertiary hover:text-primary hover:bg-surface-hover rounded transition-colors"
                  :title="$t('asset-media-add')"
                  :aria-label="$t('asset-media-add')"
                  @click="mediaPanelRef?.openPicker()"
                >
                  <Icon name="add" size="sm" />
                </button>
              </template>
              <AssetMediaPanel
                ref="mediaPanelRef"
                :asset-id="device.id"
                :can-edit="device.is_editable"
                compact
              />
            </SectionCard>
          </aside>
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
      :currentUserId="device?.primary_user_uuid ?? null"
      @close="showUserSelectionModal = false"
      @select-user="handleUserSelection"
    />

    <!-- Managed-by Selection Modal -->
    <UserSelectionModal
      :show="showManagedByModal"
      :currentUserId="device?.managed_by_user_uuid ?? null"
      @close="showManagedByModal = false"
      @select-user="handleManagedBySelection"
    />

    <!-- Unmanage Asset Confirmation Modal -->
    <Modal
      :show="showUnmanageModal"
      :title="$t('asset-detail-unmanage-modal-title')"
      @close="showUnmanageModal = false"
    >
      <div class="flex flex-col items-center gap-4">
        <div class="mx-auto flex items-center justify-center h-12 w-12 rounded-full bg-status-warning/20">
          <Icon name="refresh" size="lg" class="text-status-warning" />
        </div>
        <h3 class="text-xl font-medium text-primary">{{ $t('asset-detail-unmanage-heading') }}</h3>
        <p
          class="text-sm text-secondary text-center max-w-sm"
          v-html="$t('asset-detail-unmanage-confirm-body', { name: (device?.attributes?.hostname as string | undefined) || device?.name || '' })"
        ></p>
        <p class="text-xs text-tertiary text-center max-w-sm">{{ $t('asset-detail-unmanage-confirm-note') }}</p>
        <AlertMessage v-if="unmanageError" type="error" :message="unmanageError" />
        <div class="flex justify-center gap-3 mt-2 w-full">
          <Button block variant="secondary" @click="showUnmanageModal = false">
            {{ $t('asset-detail-action-cancel') }}
          </Button>
          <Button block variant="danger" :disabled="isSaving" :loading="isSaving" @click="confirmUnmanageDevice">
            {{ isSaving ? $t('asset-detail-action-unmanage-processing') : $t('asset-detail-unmanage-action-confirm') }}
          </Button>
        </div>
      </div>
    </Modal>
  </div>
</template>

<style scoped>
/* Two-pane shell: the property list + panels take the wide primary
 * column on the left; groups + record metadata sit in a compact rail
 * on the right. Stacks to one column on mobile, primary first. */
.asset-grid {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  width: 100%;
}
.asset-rail,
.asset-main {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  min-width: 0;
  width: 100%;
}
@media (min-width: 1024px) {
  .asset-grid {
    flex-direction: row;
    align-items: flex-start;
    gap: 1.5rem;
  }
  .asset-main {
    flex: 1 1 0;
    min-width: 0;
    order: 1;
  }
  .asset-rail {
    flex: 0 0 300px;
    max-width: 300px;
    min-width: 280px;
    position: sticky;
    top: 1rem;
    order: 2;
  }
}
</style>
