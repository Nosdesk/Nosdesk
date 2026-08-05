<script setup lang="ts">
/**
 * Native asset groups: workspace-local, color-coded collections agents curate
 * while organizing assets. Lives in the Assets area next to Inventory and
 * Catalog and is styled the same way: a full-bleed list (table on desktop,
 * cards on mobile) filtered by status, with create/edit in a compact modal.
 * Group *assignment* happens from each asset's detail page; this surface only
 * governs the group definitions.
 */
import { computed, reactive, ref } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';

import AssetViewTabs from '@/components/assets/AssetViewTabs.vue';
import DataTable, { type Column } from '@/components/common/DataTable.vue';
import Button from '@/components/common/Button.vue';
import Modal from '@/components/Modal.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import ColorHueSlider from '@/components/common/ColorHueSlider.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import ErrorBanner from '@/components/common/ErrorBanner.vue';
import { useColorFilter } from '@/composables/useColorFilter';
import {
  listAssetGroups,
  createAssetGroup,
  updateAssetGroup,
  archiveAssetGroup,
  restoreAssetGroup,
  ASSET_GROUPS_ALL_QUERY_KEY,
  type AssetGroupSummary,
} from '@/services/assetGroupService';
import { useAssetGroupsStore } from '@/stores/assetGroups';
import { useToastStore } from '@nosdesk/core/stores/toast';
import { extractErrorMessage } from '@/utils/errors';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const router = useRouter();
const toast = useToastStore();
const store = useAssetGroupsStore();
const { colorFilterStyle } = useColorFilter();

/** Drill into a group: the inventory list, pre-filtered to it (the same
 *  `groups` facet param the list reads from the URL). */
function openAssetsForGroup(group: AssetGroupSummary): void {
  router.push({ path: '/assets', query: { groups: String(group.id) } });
}

const DEFAULT_COLOR = '#6366f1';

// Cache-first list of every group (incl. archived) for the management table;
// mutations invalidate this key. The active-only picker/facet cache is the
// separate `assetGroups` store, refreshed alongside.
const queryCache = useQueryCache();
const groupsQuery = useQuery({
  key: ASSET_GROUPS_ALL_QUERY_KEY,
  query: () => listAssetGroups(true),
});
const groups = computed<AssetGroupSummary[]>(() => groupsQuery.data.value ?? []);
const isLoading = computed(
  () => groupsQuery.status.value === 'pending' && groupsQuery.data.value === undefined,
);
const loadError = computed(() =>
  groupsQuery.error.value
    ? extractErrorMessage(groupsQuery.error.value, t('admin-asset-groups-error-load'))
    : '',
);
const busyId = ref<number | null>(null);

function refreshCaches(): Promise<unknown> {
  return Promise.all([
    queryCache.invalidateQueries({ key: ASSET_GROUPS_ALL_QUERY_KEY }),
    store.load(true),
  ]);
}

// ---- Filter + sort -------------------------------------------------------
type StatusFilter = 'all' | 'active' | 'archived';
const statusFilter = ref<StatusFilter>('active');
const sortField = ref('name');
const sortDir = ref<'asc' | 'desc'>('asc');

const activeCount = computed(() => groups.value.filter((g) => !g.archived_at).length);
const archivedCount = computed(() => groups.value.filter((g) => g.archived_at).length);

const statusTabs = computed<{ value: StatusFilter; label: string; count: number }[]>(() => [
  { value: 'active', label: t('admin-asset-groups-filter-active'), count: activeCount.value },
  { value: 'all', label: t('admin-asset-groups-filter-all'), count: groups.value.length },
  { value: 'archived', label: t('admin-asset-groups-filter-archived'), count: archivedCount.value },
]);

function onSort(field: string, dir: 'asc' | 'desc') {
  sortField.value = field;
  sortDir.value = dir;
}

const displayed = computed<AssetGroupSummary[]>(() => {
  let list = groups.value;
  if (statusFilter.value === 'active') list = list.filter((g) => !g.archived_at);
  else if (statusFilter.value === 'archived') list = list.filter((g) => g.archived_at);
  const dir = sortDir.value === 'asc' ? 1 : -1;
  return [...list].sort((a, b) =>
    sortField.value === 'members'
      ? (a.asset_count - b.asset_count) * dir
      : a.name.localeCompare(b.name) * dir,
  );
});

const columns: Column[] = [
  // Bounded px maxes so `name` is the only flexible track and keeps the
  // slack. Description was `2fr` against the name column's `1fr`, which
  // handed the supporting text twice the width of the identity column
  // it supports. Minimums sum under the 768px this table gets at its
  // narrowest (a 1024px viewport less the 256px navbar); the grid clips
  // rather than scrolling if they don't.
  { field: 'name', label: t('admin-asset-groups-field-name'), width: 'minmax(180px,1fr)', sortable: true, responsive: 'always' },
  { field: 'description', label: t('admin-asset-groups-field-description'), width: 'minmax(160px,340px)', sortable: false, responsive: 'lg' },
  { field: 'members', label: t('admin-asset-groups-col-members'), width: 'minmax(90px,120px)', sortable: true, responsive: 'md' },
  { field: 'actions', label: '', width: 'minmax(120px,150px)', sortable: false, responsive: 'always' },
];

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
    if (editingId.value === null) await createAssetGroup(payload);
    else await updateAssetGroup(editingId.value, payload);
    showModal.value = false;
    await refreshCaches();
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
    await refreshCaches();
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
    await refreshCaches();
  } catch (err) {
    toast.error(extractErrorMessage(err, t('admin-asset-groups-error-restore')));
  } finally {
    busyId.value = null;
  }
}
</script>

<template>
  <div class="h-full flex flex-col">
    <!-- Header band: view tabs inline at the start, then view controls.
         Mirrors ListPageLayout (inventory) so the tab strip sits in the
         same place across Inventory / Catalog / Groups. -->
    <div class="shrink-0 bg-surface border-b border-subtle">
      <div class="px-4 py-3 flex items-center gap-2 sm:gap-3 flex-wrap">
        <AssetViewTabs />
        <div class="flex items-center gap-1 overflow-x-auto flex-1 min-w-0">
          <button
            v-for="tab in statusTabs"
            :key="tab.value"
            type="button"
            class="inline-flex items-center gap-1.5 h-7 px-2.5 rounded-md text-sm font-medium transition-colors whitespace-nowrap shrink-0"
            :class="statusFilter === tab.value ? 'bg-accent/15 text-accent' : 'text-secondary hover:bg-surface-hover'"
            @click="statusFilter = tab.value"
          >
            {{ tab.label }}
            <span class="text-xs tabular-nums" :class="statusFilter === tab.value ? 'text-accent/70' : 'text-tertiary'">{{ tab.count }}</span>
          </button>
        </div>
        <Button size="sm" icon="add" @click="openCreate">
          {{ t('admin-asset-groups-new') }}
        </Button>
      </div>
    </div>

    <!-- Content states, mirroring ListPageLayout: a hard load failure (nothing
         cached) shows the error banner with retry; otherwise the empty copy or
         the list. A failed background refresh keeps the cached rows (SWR). -->
    <div
      v-if="loadError && groups.length === 0"
      class="flex-1 flex items-center justify-center p-6"
    >
      <ErrorBanner
        class="max-w-md w-full"
        :message="loadError"
        :show-retry="true"
        @retry="() => groupsQuery.refetch()"
      />
    </div>

    <EmptyState
      v-else-if="!isLoading && groups.length === 0"
      class="flex-1"
      icon="device"
      :title="t('admin-asset-groups-empty-title')"
      :description="t('admin-asset-groups-empty-description')"
      :action-label="t('admin-asset-groups-new')"
      @action="openCreate"
    />

    <EmptyState
      v-else-if="!isLoading && displayed.length === 0"
      class="flex-1"
      icon="search"
      :title="t('admin-asset-groups-filter-empty')"
    />

    <!-- List -->
    <div v-else class="flex-1 min-h-0 overflow-auto">
      <!-- Desktop table -->
      <div class="hidden lg:block">
        <DataTable
          :columns="columns"
          :data="displayed"
          :selectable="false"
          :selected-items="[]"
          :sort-field="sortField"
          :sort-direction="sortDir"
          @update:sort="onSort"
          @row-click="(g: AssetGroupSummary) => openAssetsForGroup(g)"
        >
          <template #cell-name="{ item }">
            <div class="flex items-center gap-2.5 min-w-0">
              <span
                class="w-3 h-3 rounded-full flex-shrink-0"
                :style="{ backgroundColor: item.color || DEFAULT_COLOR, ...colorFilterStyle }"
              ></span>
              <span class="text-sm font-medium text-primary truncate">{{ item.name }}</span>
              <span
                v-if="item.archived_at"
                class="text-xs px-1.5 py-0.5 rounded bg-surface-alt text-tertiary shrink-0"
              >{{ t('admin-asset-groups-filter-archived') }}</span>
            </div>
          </template>
          <template #cell-description="{ item }">
            <span class="text-sm text-secondary truncate">{{ item.description || '-' }}</span>
          </template>
          <template #cell-members="{ item }">
            <span class="text-sm text-secondary tabular-nums">{{ item.asset_count }}</span>
          </template>
          <template #cell-actions="{ item }">
            <div class="flex items-center justify-end gap-0.5" @click.stop>
              <Button
                variant="ghost"
                size="sm"
                icon="rename"
                :aria-label="t('admin-asset-groups-action-edit')"
                :title="t('admin-asset-groups-action-edit')"
                @click="openEdit(item)"
              />
              <Button
                v-if="item.archived_at"
                variant="secondary"
                size="sm"
                :disabled="busyId === item.id"
                @click="restore(item)"
              >
                {{ t('admin-asset-groups-action-restore') }}
              </Button>
              <Button
                v-else
                variant="ghost"
                size="sm"
                icon="archive"
                :disabled="busyId === item.id"
                :aria-label="t('admin-asset-groups-action-archive')"
                :title="t('admin-asset-groups-action-archive')"
                @click="archive(item)"
              />
            </div>
          </template>
        </DataTable>
      </div>

      <!-- Mobile cards -->
      <div v-if="displayed.length > 0" class="lg:hidden grid grid-cols-1 sm:grid-cols-2 gap-3 p-4 sm:p-6">
        <div
          v-for="group in displayed"
          :key="group.id"
          class="flex items-start justify-between gap-2 bg-surface border border-default rounded-lg p-3 cursor-pointer transition-colors hover:border-strong"
          :class="{ 'opacity-60': group.archived_at }"
          @click="openAssetsForGroup(group)"
        >
          <div class="min-w-0 flex flex-col gap-0.5">
            <div class="flex items-center gap-2 min-w-0">
              <span
                class="w-3 h-3 rounded-full flex-shrink-0"
                :style="{ backgroundColor: group.color || DEFAULT_COLOR, ...colorFilterStyle }"
              ></span>
              <span class="text-sm font-medium text-primary truncate">{{ group.name }}</span>
            </div>
            <div class="text-xs text-tertiary truncate">
              {{ t('admin-asset-groups-member-count', { count: group.asset_count }) }}<template v-if="group.description"> · {{ group.description }}</template>
            </div>
          </div>
          <div class="flex items-center gap-0.5 shrink-0" @click.stop>
            <Button
              variant="ghost"
              size="sm"
              icon="rename"
              :aria-label="t('admin-asset-groups-action-edit')"
              :title="t('admin-asset-groups-action-edit')"
              @click="openEdit(group)"
            />
            <Button
              v-if="group.archived_at"
              variant="secondary"
              size="sm"
              :disabled="busyId === group.id"
              @click="restore(group)"
            >
              {{ t('admin-asset-groups-action-restore') }}
            </Button>
            <Button
              v-else
              variant="ghost"
              size="sm"
              icon="archive"
              :disabled="busyId === group.id"
              :aria-label="t('admin-asset-groups-action-archive')"
              @click="archive(group)"
            />
          </div>
        </div>
      </div>
    </div>

    <!-- Create / edit modal -->
    <Modal
      :show="showModal"
      :title="editingId === null ? t('admin-asset-groups-modal-create-title') : t('admin-asset-groups-modal-edit-title')"
      size="sm"
      @close="showModal = false"
    >
      <div class="flex flex-col gap-3">
        <FormInput
          v-model="form.name"
          :label="t('admin-asset-groups-field-name')"
          :placeholder="t('admin-asset-groups-field-name-placeholder')"
          size="sm"
          @keyup.enter="save"
        />
        <FormTextarea
          v-model="form.description"
          :label="t('admin-asset-groups-field-description')"
          :placeholder="t('admin-asset-groups-field-description-placeholder')"
          :rows="2"
        />
        <ColorHueSlider v-model="form.color" :label="t('admin-asset-groups-field-color')" />
        <div class="flex justify-end gap-2">
          <Button variant="secondary" :disabled="saving" @click="showModal = false">
            {{ t('common-cancel') }}
          </Button>
          <Button :disabled="!canSave" :loading="saving" @click="save">
            {{ t('common-save') }}
          </Button>
        </div>
      </div>
    </Modal>
  </div>
</template>
