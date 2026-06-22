<script setup lang="ts">
/**
 * Asset catalog: the make/model library (NetBox-style device types).
 * Operational reference data agents curate while cataloging assets, so
 * it lives in the Assets area next to Inventory and Planner, styled as a
 * list view: models are the primary table (full-bleed on desktop, cards
 * on mobile) filtered by manufacturer; manufacturers are managed in a
 * compact modal. Default-spec editing is a later refinement.
 */
import { computed, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQueryCache } from '@pinia/colada';

import AssetViewTabs from '@/components/assets/AssetViewTabs.vue';
import DataTable, { type Column } from '@/components/common/DataTable.vue';
import Button from '@/components/common/Button.vue';
import Modal from '@/components/Modal.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import SearchableDropdown, { type DropdownOption } from '@/components/common/SearchableDropdown.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import DynamicAttributeForm from '@/components/assets/DynamicAttributeForm.vue';
import { userAttributeSchema } from '@/components/assets/assetAttributeSchema';

import { useManufacturersQuery, useAssetModelsQuery } from '@/composables/useAssetCatalogQuery';
import { useAssetKindsQuery } from '@/composables/useAssetKindsQuery';
import {
  manufacturersService,
  assetModelsService,
  MANUFACTURERS_QUERY_KEY,
  ASSET_MODELS_QUERY_KEY,
} from '@/services/assetCatalogService';
import { extractErrorMessage } from '@/utils/errors';
import { useToastStore } from '@/stores/toast';
import type { AssetModel } from '@/types/asset';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const toast = useToastStore();
const queryCache = useQueryCache();

const { manufacturers } = useManufacturersQuery();
const { models } = useAssetModelsQuery();
const { kinds } = useAssetKindsQuery();

const manufacturerName = (id: number) => manufacturers.value.find((m) => m.id === id)?.name ?? '';
const kindLabel = (slug: string) => kinds.value.find((k) => k.slug === slug)?.label ?? slug;
const modelCount = (manufacturerId: number) =>
  models.value.filter((m) => m.manufacturer_id === manufacturerId).length;

async function refresh() {
  await queryCache.invalidateQueries({ key: MANUFACTURERS_QUERY_KEY });
  await queryCache.invalidateQueries({ key: ASSET_MODELS_QUERY_KEY });
}

// ---- Filter + sort -----------------------------------------------
const manufacturerFilter = ref<number | 'all'>('all');
const sortField = ref('name');
const sortDir = ref<'asc' | 'desc'>('asc');

function onSort(field: string, dir: 'asc' | 'desc') {
  sortField.value = field;
  sortDir.value = dir;
}

const displayedModels = computed(() => {
  let list = models.value;
  if (manufacturerFilter.value !== 'all') {
    list = list.filter((m) => m.manufacturer_id === manufacturerFilter.value);
  }
  const dir = sortDir.value === 'asc' ? 1 : -1;
  return [...list].sort((a, b) => {
    let av: string;
    let bv: string;
    if (sortField.value === 'manufacturer') {
      av = manufacturerName(a.manufacturer_id);
      bv = manufacturerName(b.manufacturer_id);
    } else if (sortField.value === 'kind') {
      av = kindLabel(a.kind);
      bv = kindLabel(b.kind);
    } else {
      av = a.name;
      bv = b.name;
    }
    return av.localeCompare(bv) * dir;
  });
});

const columns: Column[] = [
  { field: 'name', label: t('asset-catalog-col-model'), width: '1fr', sortable: true, responsive: 'always' },
  { field: 'manufacturer', label: t('asset-model-field-manufacturer'), width: 'minmax(140px,auto)', sortable: true, responsive: 'always' },
  { field: 'kind', label: t('asset-catalog-col-type'), width: 'minmax(120px,auto)', sortable: true, responsive: 'md' },
  { field: 'part_number', label: t('asset-catalog-col-part-number'), width: 'minmax(120px,auto)', sortable: false, responsive: 'lg' },
  { field: 'actions', label: '', width: 'minmax(56px,auto)', sortable: false, responsive: 'always' },
];

// ---- Model modal -------------------------------------------------
interface ModelForm {
  show: boolean;
  id: number | null;
  manufacturer_id: string;
  name: string;
  kind: string;
  part_number: string;
  notes: string;
  default_attributes: Record<string, unknown>;
}
const blankModel = (): ModelForm => ({
  show: false,
  id: null,
  manufacturer_id: '',
  name: '',
  kind: 'generic',
  part_number: '',
  notes: '',
  default_attributes: {},
});
const modelModal = ref<ModelForm>(blankModel());
const modelSaving = ref(false);
const modelError = ref<string | null>(null);

const manufacturerOptions = computed<DropdownOption[]>(() =>
  manufacturers.value.map((m) => ({ value: String(m.id), label: m.name })),
);
const kindOptions = computed<DropdownOption[]>(() =>
  kinds.value.map((k) => ({ value: k.slug, label: k.label, icon: k.icon ?? undefined })),
);
const modelCanSave = computed(
  () =>
    modelModal.value.manufacturer_id !== '' &&
    modelModal.value.name.trim() !== '' &&
    modelModal.value.kind !== '',
);

// Default specs are authored against the user-editable slice of the
// chosen kind's schema (sync-owned Intune/Entra keys are never defaulted).
const modelKindUserSchema = computed(() => {
  const schema = kinds.value.find((k) => k.slug === modelModal.value.kind)
    ?.attribute_schema as Record<string, unknown> | undefined;
  return userAttributeSchema(schema ?? null);
});

// Switching kinds drops any drafted specs the new kind doesn't define,
// so a model never carries defaults that wouldn't stamp a valid asset.
function onModelKindChange(slug: string) {
  modelModal.value.kind = slug;
  const allowed = new Set(
    Object.keys((modelKindUserSchema.value?.properties as Record<string, unknown>) ?? {}),
  );
  const pruned: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(modelModal.value.default_attributes)) {
    if (allowed.has(key)) pruned[key] = value;
  }
  modelModal.value.default_attributes = pruned;
}

function openModelCreate() {
  modelModal.value = {
    ...blankModel(),
    show: true,
    manufacturer_id: manufacturerFilter.value !== 'all' ? String(manufacturerFilter.value) : '',
  };
  modelError.value = null;
}
function openModelEdit(m: AssetModel) {
  modelModal.value = {
    show: true,
    id: m.id,
    manufacturer_id: String(m.manufacturer_id),
    name: m.name,
    kind: m.kind,
    part_number: m.part_number ?? '',
    notes: m.notes ?? '',
    default_attributes: { ...(m.default_attributes ?? {}) },
  };
  modelError.value = null;
}
async function saveModel() {
  if (!modelCanSave.value) return;
  modelSaving.value = true;
  modelError.value = null;
  const f = modelModal.value;
  try {
    const body = {
      manufacturer_id: Number(f.manufacturer_id),
      name: f.name.trim(),
      kind: f.kind,
      part_number: f.part_number.trim() || null,
      notes: f.notes.trim() || null,
      default_attributes: f.default_attributes,
    };
    if (f.id == null) {
      await assetModelsService.create(body);
    } else {
      await assetModelsService.update(f.id, body);
    }
    await refresh();
    modelModal.value.show = false;
  } catch (e) {
    modelError.value = extractErrorMessage(e, t('asset-catalog-model-save-failed'));
  } finally {
    modelSaving.value = false;
  }
}

// ---- Manufacturers manage modal ----------------------------------
const manageOpen = ref(false);
const newMfrName = ref('');
const manageError = ref<string | null>(null);

async function addMfr() {
  const name = newMfrName.value.trim();
  if (!name) return;
  manageError.value = null;
  try {
    await manufacturersService.create({ name });
    newMfrName.value = '';
    await refresh();
  } catch (e) {
    manageError.value = extractErrorMessage(e, t('asset-catalog-manufacturer-save-failed'));
  }
}
async function renameMfr(id: number, value: string) {
  const name = value.trim();
  if (!name) return;
  manageError.value = null;
  try {
    await manufacturersService.update(id, { name });
    await refresh();
  } catch (e) {
    manageError.value = extractErrorMessage(e, t('asset-catalog-manufacturer-save-failed'));
  }
}
async function deleteMfr(id: number) {
  manageError.value = null;
  try {
    await manufacturersService.delete(id);
    if (manufacturerFilter.value === id) manufacturerFilter.value = 'all';
    await refresh();
  } catch (e) {
    manageError.value = extractErrorMessage(e, t('asset-catalog-delete-failed'));
  }
}

// ---- Model delete confirm ----------------------------------------
const deleteTarget = ref<AssetModel | null>(null);
const deleteConfirm = computed(() =>
  deleteTarget.value
    ? t('asset-catalog-model-delete-confirm', { name: deleteTarget.value.name })
    : '',
);
async function confirmDelete() {
  const target = deleteTarget.value;
  deleteTarget.value = null;
  if (!target) return;
  try {
    await assetModelsService.delete(target.id);
    await refresh();
  } catch (e) {
    toast.error(extractErrorMessage(e, t('asset-catalog-delete-failed')));
  }
}
</script>

<template>
  <div class="h-full flex flex-col">
    <div class="px-4 sm:px-6 pt-3">
      <AssetViewTabs />
    </div>

    <!-- Toolbar: manufacturer filter chips + actions -->
    <div class="shrink-0 flex items-center gap-2 px-4 sm:px-6 py-2.5 border-b border-subtle">
      <div class="flex items-center gap-1 overflow-x-auto flex-1 min-w-0">
        <button
          type="button"
          class="inline-flex items-center gap-1.5 h-7 px-2.5 rounded-md text-sm font-medium transition-colors whitespace-nowrap shrink-0"
          :class="manufacturerFilter === 'all' ? 'bg-accent/15 text-accent' : 'text-secondary hover:bg-surface-hover'"
          @click="manufacturerFilter = 'all'"
        >
          {{ $t('asset-catalog-filter-all') }}
          <span class="text-xs tabular-nums" :class="manufacturerFilter === 'all' ? 'text-accent/70' : 'text-tertiary'">{{ models.length }}</span>
        </button>
        <button
          v-for="m in manufacturers"
          :key="m.id"
          type="button"
          class="inline-flex items-center gap-1.5 h-7 px-2.5 rounded-md text-sm font-medium transition-colors whitespace-nowrap shrink-0"
          :class="manufacturerFilter === m.id ? 'bg-accent/15 text-accent' : 'text-secondary hover:bg-surface-hover'"
          @click="manufacturerFilter = m.id"
        >
          {{ m.name }}
          <span class="text-xs tabular-nums" :class="manufacturerFilter === m.id ? 'text-accent/70' : 'text-tertiary'">{{ modelCount(m.id) }}</span>
        </button>
      </div>
      <Button variant="secondary" size="sm" icon="settings" @click="manageOpen = true">
        {{ $t('asset-catalog-manage-manufacturers') }}
      </Button>
      <Button
        size="sm"
        icon="add"
        :disabled="manufacturers.length === 0"
        :title="manufacturers.length === 0 ? $t('asset-catalog-need-manufacturer') : undefined"
        @click="openModelCreate"
      >
        {{ $t('asset-catalog-add-model') }}
      </Button>
    </div>

    <!-- Empty -->
    <EmptyState
      v-if="models.length === 0"
      class="flex-1"
      icon="device"
      :title="$t('asset-catalog-models-empty')"
      :description="manufacturers.length === 0 ? $t('asset-catalog-need-manufacturer') : undefined"
      :action-label="manufacturers.length > 0 ? $t('asset-catalog-add-model') : $t('asset-catalog-manage-manufacturers')"
      @action="manufacturers.length > 0 ? openModelCreate() : (manageOpen = true)"
    />

    <!-- List -->
    <div v-else class="flex-1 min-h-0 overflow-auto">
      <!-- Desktop table -->
      <div class="hidden lg:block">
        <DataTable
          :columns="columns"
          :data="displayedModels"
          :selectable="false"
          :selected-items="[]"
          :sort-field="sortField"
          :sort-direction="sortDir"
          @update:sort="onSort"
          @row-click="(m: AssetModel) => openModelEdit(m)"
        >
          <template #cell-name="{ item }">
            <span class="text-sm font-medium text-primary truncate">{{ item.name }}</span>
          </template>
          <template #cell-manufacturer="{ item }">
            <span class="text-sm text-secondary">{{ manufacturerName(item.manufacturer_id) }}</span>
          </template>
          <template #cell-kind="{ item }">
            <span class="text-xs font-medium text-secondary">{{ kindLabel(item.kind) }}</span>
          </template>
          <template #cell-part_number="{ item }">
            <span class="text-xs font-mono text-tertiary">{{ item.part_number || '-' }}</span>
          </template>
          <template #cell-actions="{ item }">
            <div class="flex items-center justify-end" @click.stop>
              <Button
                variant="ghost"
                size="sm"
                icon="trash"
                :aria-label="$t('common-delete')"
                @click="deleteTarget = item"
              />
            </div>
          </template>
        </DataTable>
      </div>

      <!-- Mobile cards -->
      <div class="lg:hidden grid grid-cols-1 sm:grid-cols-2 gap-3 p-4 sm:p-6">
        <div
          v-for="m in displayedModels"
          :key="m.id"
          class="group flex items-start justify-between gap-2 bg-surface border border-default rounded-lg p-3 cursor-pointer transition-colors hover:border-strong"
          @click="openModelEdit(m)"
        >
          <div class="min-w-0">
            <div class="text-sm font-medium text-primary truncate">{{ m.name }}</div>
            <div class="text-xs text-tertiary mt-0.5 truncate">
              {{ manufacturerName(m.manufacturer_id) }} · {{ kindLabel(m.kind) }}<template v-if="m.part_number"> · {{ m.part_number }}</template>
            </div>
          </div>
          <Button
            variant="ghost"
            size="sm"
            icon="trash"
            :aria-label="$t('common-delete')"
            @click.stop="deleteTarget = m"
          />
        </div>
      </div>
    </div>

    <!-- Model modal -->
    <Modal
      :show="modelModal.show"
      :title="modelModal.id == null ? $t('asset-catalog-add-model') : $t('asset-catalog-edit-model')"
      size="md"
      @close="modelModal.show = false"
    >
      <div class="flex flex-col gap-3">
        <div class="flex flex-col gap-1.5">
          <label class="text-xs font-medium uppercase tracking-wide text-tertiary">
            {{ $t('asset-model-field-manufacturer') }}
          </label>
          <SearchableDropdown
            :model-value="modelModal.manufacturer_id"
            :options="manufacturerOptions"
            :placeholder="$t('asset-model-field-manufacturer-placeholder')"
            size="sm"
            @update:model-value="(v) => (modelModal.manufacturer_id = String(v))"
          />
        </div>
        <FormInput
          v-model="modelModal.name"
          :label="$t('asset-model-field-name')"
          :placeholder="$t('asset-model-field-name-placeholder')"
          size="sm"
        />
        <div class="flex flex-col gap-1.5">
          <label class="text-xs font-medium uppercase tracking-wide text-tertiary">
            {{ $t('asset-detail-field-kind') }}
          </label>
          <SearchableDropdown
            :model-value="modelModal.kind"
            :options="kindOptions"
            size="sm"
            @update:model-value="(v) => onModelKindChange(String(v))"
          />
        </div>
        <FormInput
          v-model="modelModal.part_number"
          :label="$t('asset-catalog-part-number')"
          :placeholder="$t('asset-catalog-part-number-placeholder')"
          size="sm"
        />

        <!-- Default specs: pre-fill stamped onto every asset of this model -->
        <div v-if="modelKindUserSchema" class="flex flex-col gap-1.5 pt-1 border-t border-subtle">
          <label class="text-xs font-medium uppercase tracking-wide text-tertiary">
            {{ $t('asset-catalog-default-specs') }}
          </label>
          <p class="text-xs text-tertiary -mt-0.5">{{ $t('asset-catalog-default-specs-hint') }}</p>
          <DynamicAttributeForm
            v-model="modelModal.default_attributes"
            :schema="modelKindUserSchema"
          />
        </div>

        <FormTextarea v-model="modelModal.notes" :label="$t('asset-catalog-notes')" :rows="2" :max-rows="6" />
        <AlertMessage v-if="modelError" type="error" :message="modelError" />
        <div class="flex justify-end gap-2">
          <Button variant="secondary" :disabled="modelSaving" @click="modelModal.show = false">
            {{ $t('common-cancel') }}
          </Button>
          <Button :disabled="!modelCanSave || modelSaving" :loading="modelSaving" @click="saveModel">
            {{ $t('common-save') }}
          </Button>
        </div>
      </div>
    </Modal>

    <!-- Manage manufacturers modal -->
    <Modal :show="manageOpen" :title="$t('asset-catalog-manage-manufacturers')" size="sm" @close="manageOpen = false">
      <div class="flex flex-col gap-3">
        <div class="flex items-center gap-2">
          <FormInput
            v-model="newMfrName"
            class="flex-1"
            :placeholder="$t('asset-catalog-manufacturer-name-placeholder')"
            size="sm"
            @keyup.enter="addMfr"
          />
          <Button size="sm" icon="add" :disabled="!newMfrName.trim()" @click="addMfr">
            {{ $t('common-save') }}
          </Button>
        </div>
        <AlertMessage v-if="manageError" type="error" :message="manageError" />
        <p v-if="manufacturers.length === 0" class="text-sm text-tertiary py-2 text-center">
          {{ $t('asset-catalog-manufacturers-empty') }}
        </p>
        <ul v-else class="flex flex-col divide-y divide-default max-h-80 overflow-y-auto">
          <li v-for="m in manufacturers" :key="m.id" class="flex items-center gap-2 py-1.5">
            <input
              class="flex-1 min-w-0 bg-transparent text-sm text-primary border border-transparent hover:border-default focus:border-accent rounded px-1.5 py-0.5 focus:outline-none transition-colors"
              :value="m.name"
              :aria-label="$t('asset-catalog-manufacturer-name')"
              @change="(e) => renameMfr(m.id, (e.target as HTMLInputElement).value)"
            />
            <span class="text-xs text-tertiary whitespace-nowrap">
              {{ $t('asset-catalog-model-count', { count: modelCount(m.id) }) }}
            </span>
            <Button variant="ghost" size="sm" icon="trash" :aria-label="$t('common-delete')" @click="deleteMfr(m.id)" />
          </li>
        </ul>
      </div>
    </Modal>

    <ConfirmModal
      :show="deleteTarget != null"
      variant="danger"
      :title="$t('asset-catalog-delete-title')"
      :message="deleteConfirm"
      :confirm-label="$t('common-delete')"
      @confirm="confirmDelete"
      @close="deleteTarget = null"
    />
  </div>
</template>
