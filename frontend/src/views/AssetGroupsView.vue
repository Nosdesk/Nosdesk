<script setup lang="ts">
/**
 * Admin management of native asset groups: create / rename / recolor /
 * archive / restore, with live member counts. Group *assignment* happens from
 * the asset detail; this surface only governs the group definitions.
 *
 * Keeps its own list (active + archived) rather than the picker store, then
 * refreshes that store after each mutation so the asset-list facet and detail
 * picker stay current.
 */
import { computed, onMounted, reactive, ref } from 'vue';
import { useFluent } from 'fluent-vue';

import AssetViewTabs from '@/components/assets/AssetViewTabs.vue';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import ColorHueSlider from '@/components/common/ColorHueSlider.vue';
import Modal from '@/components/Modal.vue';
import Icon from '@/components/common/Icon.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import {
  listAssetGroups,
  createAssetGroup,
  updateAssetGroup,
  archiveAssetGroup,
  restoreAssetGroup,
  type AssetGroupSummary,
} from '@/services/assetGroupService';
import { useAssetGroupsStore } from '@/stores/assetGroups';
import { useToastStore } from '@/stores/toast';
import { extractErrorMessage } from '@/utils/errors';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const toast = useToastStore();
const store = useAssetGroupsStore();

const DEFAULT_COLOR = '#6366f1';

const groups = ref<AssetGroupSummary[]>([]);
const isLoading = ref(true);
const loadError = ref('');
const busyId = ref<number | null>(null);

const active = computed(() => groups.value.filter((g) => !g.archived_at));
const archived = computed(() => groups.value.filter((g) => g.archived_at));

async function refresh(): Promise<void> {
  try {
    groups.value = await listAssetGroups(true);
    loadError.value = '';
  } catch (err) {
    loadError.value = extractErrorMessage(err, t('admin-asset-groups-error-load'));
  } finally {
    isLoading.value = false;
  }
}

onMounted(refresh);

// ---- Create / edit modal -------------------------------------------------

const showModal = ref(false);
const saving = ref(false);
const editingId = ref<number | null>(null);
const form = reactive({ name: '', description: '', color: DEFAULT_COLOR });

function openCreate(): void {
  editingId.value = null;
  form.name = '';
  form.description = '';
  form.color = DEFAULT_COLOR;
  showModal.value = true;
}

function openEdit(group: AssetGroupSummary): void {
  editingId.value = group.id;
  form.name = group.name;
  form.description = group.description ?? '';
  form.color = group.color || DEFAULT_COLOR;
  showModal.value = true;
}

const canSave = computed(() => form.name.trim().length > 0 && !saving.value);

async function save(): Promise<void> {
  if (!canSave.value) return;
  saving.value = true;
  try {
    const payload = {
      name: form.name.trim(),
      description: form.description.trim() || null,
      color: form.color,
    };
    if (editingId.value === null) {
      await createAssetGroup(payload);
    } else {
      await updateAssetGroup(editingId.value, payload);
    }
    showModal.value = false;
    await Promise.all([refresh(), store.load(true)]);
  } catch (err) {
    toast.error(extractErrorMessage(err, t('admin-asset-groups-error-save')));
  } finally {
    saving.value = false;
  }
}

// ---- Archive / restore ---------------------------------------------------

async function archive(group: AssetGroupSummary): Promise<void> {
  busyId.value = group.id;
  try {
    await archiveAssetGroup(group.id);
    await Promise.all([refresh(), store.load(true)]);
  } catch (err) {
    toast.error(extractErrorMessage(err, t('admin-asset-groups-error-archive')));
  } finally {
    busyId.value = null;
  }
}

async function restore(group: AssetGroupSummary): Promise<void> {
  busyId.value = group.id;
  try {
    await restoreAssetGroup(group.id);
    await Promise.all([refresh(), store.load(true)]);
  } catch (err) {
    toast.error(extractErrorMessage(err, t('admin-asset-groups-error-restore')));
  } finally {
    busyId.value = null;
  }
}
</script>

<template>
  <div class="flex-1">
    <div class="px-4 sm:px-6 pt-4">
      <AssetViewTabs />
    </div>
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-5xl">
      <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
        <div class="flex flex-col gap-1">
          <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ t('admin-asset-groups-title') }}</h1>
          <p class="text-secondary text-sm sm:text-base">{{ t('admin-asset-groups-description') }}</p>
        </div>
        <Button icon="add" class="self-start sm:self-auto" @click="openCreate">
          {{ t('admin-asset-groups-new') }}
        </Button>
      </div>

      <AlertMessage v-if="loadError" type="error" :message="loadError" />

      <EmptyState
        v-if="!isLoading && groups.length === 0"
        icon="device"
        :title="t('admin-asset-groups-empty-title')"
        :description="t('admin-asset-groups-empty-description')"
        :action-label="t('admin-asset-groups-new')"
        variant="card"
        @action="openCreate"
      />

      <!-- Active groups -->
      <div v-if="active.length > 0" class="bg-surface border border-default rounded-lg overflow-hidden">
        <div
          v-for="(group, i) in active"
          :key="group.id"
          :class="[
            'flex items-center gap-3 p-3 sm:p-4',
            i < active.length - 1 ? 'border-b border-default' : '',
          ]"
        >
          <span
            class="w-3.5 h-3.5 rounded-full flex-shrink-0"
            :style="{ backgroundColor: group.color || DEFAULT_COLOR }"
          ></span>
          <div class="flex-1 min-w-0 flex flex-col gap-0.5">
            <span class="font-medium text-primary truncate">{{ group.name }}</span>
            <span v-if="group.description" class="text-sm text-secondary truncate">{{ group.description }}</span>
          </div>
          <span class="text-xs text-tertiary whitespace-nowrap">
            {{ t('admin-asset-groups-member-count', { count: group.asset_count }) }}
          </span>
          <button
            class="p-1.5 text-secondary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
            :title="t('admin-asset-groups-action-edit')"
            :aria-label="t('admin-asset-groups-action-edit')"
            @click="openEdit(group)"
          >
            <Icon name="rename" />
          </button>
          <button
            class="p-1.5 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-md transition-colors disabled:opacity-50"
            :title="t('admin-asset-groups-action-archive')"
            :aria-label="t('admin-asset-groups-action-archive')"
            :disabled="busyId === group.id"
            @click="archive(group)"
          >
            <Icon name="archive" />
          </button>
        </div>
      </div>

      <!-- Archived groups -->
      <div v-if="archived.length > 0" class="flex flex-col gap-2">
        <h2 class="text-sm font-medium uppercase tracking-wide text-secondary">
          {{ t('admin-asset-groups-archived-heading') }}
        </h2>
        <div class="bg-surface border border-default rounded-lg overflow-hidden">
          <div
            v-for="(group, i) in archived"
            :key="group.id"
            :class="[
              'flex items-center gap-3 p-3 sm:p-4 opacity-60',
              i < archived.length - 1 ? 'border-b border-default' : '',
            ]"
          >
            <span
              class="w-3.5 h-3.5 rounded-full flex-shrink-0"
              :style="{ backgroundColor: group.color || DEFAULT_COLOR }"
            ></span>
            <span class="flex-1 min-w-0 font-medium text-primary truncate">{{ group.name }}</span>
            <Button
              variant="secondary"
              size="sm"
              :disabled="busyId === group.id"
              @click="restore(group)"
            >
              {{ t('admin-asset-groups-action-restore') }}
            </Button>
          </div>
        </div>
      </div>
    </div>

    <Modal
      :show="showModal"
      :title="editingId === null ? t('admin-asset-groups-modal-create-title') : t('admin-asset-groups-modal-edit-title')"
      @close="showModal = false"
    >
      <form class="flex flex-col gap-4" @submit.prevent="save">
        <FormInput
          v-model="form.name"
          :label="t('admin-asset-groups-field-name')"
          :placeholder="t('admin-asset-groups-field-name-placeholder')"
          required
        />
        <FormTextarea
          v-model="form.description"
          :label="t('admin-asset-groups-field-description')"
          :placeholder="t('admin-asset-groups-field-description-placeholder')"
          :rows="2"
        />
        <ColorHueSlider v-model="form.color" :label="t('admin-asset-groups-field-color')" />
      </form>
      <template #footer>
        <Button variant="secondary" @click="showModal = false">{{ t('common-cancel') }}</Button>
        <Button :disabled="!canSave" :loading="saving" @click="save">{{ t('common-save') }}</Button>
      </template>
    </Modal>
  </div>
</template>
