<script setup lang="ts">
/**
 * Admin list of asset record types: the kinds of work recorded on assets.
 * The workspace's types with how many records use each, archived ones on a
 * toggle, and the built-in types not yet added, offered from the catalogue
 * rather than seeded. Editing lives on AssetRecordTypeEditView.
 */
import { computed, ref } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';

import AlertMessage from '@/components/common/AlertMessage.vue';
import BackButton from '@/components/common/BackButton.vue';
import Button from '@/components/common/Button.vue';
import Icon from '@/components/common/Icon.vue';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import type { IconName } from '@/components/common/icons';
import { useWorkspaceQuery, useWorkspaceQueryCache } from '@/queries/workspaceQuery';
import { extractErrorMessage } from '@/utils/errors';
import { useToastStore } from '@nosdesk/core/stores/toast';
import { assetRecordService, assetRecordKeys } from '@nosdesk/core/services/assetRecordService';
import {
  assetRecordTypeKeys,
  assetRecordTypeService,
} from '@nosdesk/core/services/assetRecordTypeService';
import type {
  AdminAssetRecordType,
  AssetRecordCatalogueEntry,
} from '@nosdesk/core/types/asset';
import {
  catalogueOutcomeName,
  recordTypeName,
} from '@/components/admin/assetRecordTypes/recordTypeForm';

const { $t: t } = useFluent();
const router = useRouter();
const toast = useToastStore();
const queryCache = useWorkspaceQueryCache();

const typesQuery = useWorkspaceQuery({
  key: [...assetRecordTypeKeys.root],
  query: () => assetRecordTypeService.list(),
});
const catalogueQuery = useWorkspaceQuery({
  key: [...assetRecordKeys.catalogue],
  query: () => assetRecordService.catalogue(),
});

const types = computed<AdminAssetRecordType[]>(() => typesQuery.data.value ?? []);
const activeTypes = computed(() => types.value.filter((ty) => !ty.archived_at));
const archivedTypes = computed(() => types.value.filter((ty) => ty.archived_at));
const showArchived = ref(false);

/** Built-ins the workspace hasn't added; an archived one counts as added
 *  and is restored from the archived list instead. */
const offered = computed<AssetRecordCatalogueEntry[]>(() =>
  (catalogueQuery.data.value ?? []).filter((entry) => !entry.added),
);

const loadError = computed(() =>
  typesQuery.error.value ? t('admin-asset-record-types-error-load') : '',
);
const loaded = computed(() => typesQuery.data.value !== undefined);

const adding = ref<string | null>(null);

async function refresh(): Promise<void> {
  await Promise.all([
    queryCache.invalidateQueries({ key: [...assetRecordTypeKeys.root] }),
    queryCache.invalidateQueries({ key: [...assetRecordKeys.types] }),
  ]);
}

async function addFromCatalogue(entry: AssetRecordCatalogueEntry): Promise<void> {
  if (adding.value) return;
  adding.value = entry.slug;
  try {
    await assetRecordService.addFromCatalogue(entry.slug);
    toast.success(
      t('admin-asset-record-types-added', { name: t(`asset-record-type-${entry.slug}`) }),
    );
    await refresh();
  } catch (error) {
    toast.error(extractErrorMessage(error, t('admin-asset-record-types-error-add')));
  } finally {
    adding.value = null;
  }
}

function iconOf(name: string | null | undefined): IconName {
  return (name || 'clipboardCheck') as IconName;
}

function openCreate(): void {
  router.push({ name: 'admin-asset-record-types-new' });
}
function openEdit(type: AdminAssetRecordType): void {
  router.push({ name: 'admin-asset-record-types-edit', params: { id: type.id } });
}
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-5xl">
      <div class="flex flex-col gap-2">
        <BackButton
          :fallback-route="'/admin'"
          :label="t('admin-asset-record-types-back-label')"
          compact
        />
        <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
          <div class="flex flex-col gap-1">
            <h1 class="text-xl sm:text-2xl font-bold text-primary">
              {{ t('admin-asset-record-types-title') }}
            </h1>
            <p class="text-secondary text-sm sm:text-base">
              {{ t('admin-asset-record-types-description') }}
            </p>
          </div>
          <Button icon="add" class="self-start sm:self-auto" @click="openCreate">
            {{ t('admin-asset-record-types-new') }}
          </Button>
        </div>
      </div>

      <AlertMessage v-if="loadError && types.length === 0" type="error" :message="loadError" />

      <section
        class="flex flex-col gap-2"
        :aria-label="t('admin-asset-record-types-workspace-heading')"
      >
        <div class="flex flex-wrap items-center gap-3">
          <h2 class="text-sm font-medium uppercase tracking-wide text-secondary">
            {{ t('admin-asset-record-types-workspace-heading') }}
          </h2>
          <span class="text-xs text-tertiary">{{ activeTypes.length }}</span>
          <ToggleSwitch
            v-if="archivedTypes.length > 0"
            v-model="showArchived"
            size="sm"
            class="ml-auto"
            :label="t('admin-asset-record-types-show-archived', { count: archivedTypes.length })"
          />
        </div>

        <p v-if="loaded && activeTypes.length === 0" class="text-sm text-tertiary">
          {{ t('admin-asset-record-types-none') }}
        </p>
        <ul
          v-else-if="activeTypes.length > 0"
          class="flex flex-col bg-surface border border-default rounded-lg overflow-hidden"
        >
          <li
            v-for="(type, i) in activeTypes"
            :key="type.id"
            :class="i < activeTypes.length - 1 ? 'border-b border-default' : ''"
          >
            <button
              type="button"
              data-test="record-type-row"
              class="w-full flex items-center gap-3 p-3 sm:p-4 text-left hover:bg-surface-hover transition-colors"
              @click="openEdit(type)"
            >
              <span
                class="flex items-center justify-center w-8 h-8 rounded-md bg-surface-alt text-secondary shrink-0"
                :style="type.color ? { color: type.color } : undefined"
              >
                <Icon :name="iconOf(type.icon)" />
              </span>
              <span class="flex-1 min-w-0 flex flex-col gap-0.5">
                <span class="flex flex-wrap items-center gap-2">
                  <span class="font-medium text-primary truncate">{{ recordTypeName(t, type) }}</span>
                  <code class="text-xs text-tertiary">{{ type.slug }}</code>
                  <span
                    v-if="type.is_builtin"
                    class="text-xs px-1.5 py-0.5 rounded bg-surface-alt text-tertiary"
                  >
                    {{ t('admin-asset-record-types-builtin-tag') }}
                  </span>
                </span>
                <span v-if="type.description" class="text-sm text-secondary truncate">
                  {{ type.description }}
                </span>
              </span>
              <span class="text-xs text-tertiary shrink-0">
                {{ t('admin-asset-record-types-record-count', { count: type.record_count }) }}
              </span>
            </button>
          </li>
        </ul>
      </section>

      <section
        v-if="showArchived && archivedTypes.length > 0"
        class="flex flex-col gap-2"
        :aria-label="t('admin-asset-record-types-archived-heading')"
      >
        <div class="flex flex-col gap-1">
          <h2 class="text-sm font-medium uppercase tracking-wide text-secondary">
            {{ t('admin-asset-record-types-archived-heading') }}
          </h2>
          <p class="text-xs text-tertiary">{{ t('admin-asset-record-types-archived-hint') }}</p>
        </div>
        <ul class="flex flex-col bg-surface border border-default rounded-lg overflow-hidden">
          <li
            v-for="(type, i) in archivedTypes"
            :key="type.id"
            :class="i < archivedTypes.length - 1 ? 'border-b border-default' : ''"
          >
            <button
              type="button"
              data-test="record-type-row"
              class="w-full flex items-center gap-3 p-3 sm:p-4 text-left hover:bg-surface-hover transition-colors"
              @click="openEdit(type)"
            >
              <span
                class="flex items-center justify-center w-8 h-8 rounded-md bg-surface-alt text-tertiary shrink-0"
              >
                <Icon :name="iconOf(type.icon)" />
              </span>
              <span class="flex-1 min-w-0 flex flex-wrap items-center gap-2">
                <span class="font-medium text-secondary truncate">{{ recordTypeName(t, type) }}</span>
                <code class="text-xs text-tertiary">{{ type.slug }}</code>
              </span>
              <span class="text-xs text-tertiary shrink-0">
                {{ t('admin-asset-record-types-record-count', { count: type.record_count }) }}
              </span>
            </button>
          </li>
        </ul>
      </section>

      <section
        v-if="offered.length > 0"
        class="flex flex-col gap-2"
        :aria-label="t('admin-asset-record-types-catalogue-heading')"
      >
        <div class="flex flex-col gap-1">
          <h2 class="text-sm font-medium uppercase tracking-wide text-secondary">
            {{ t('admin-asset-record-types-catalogue-heading') }}
          </h2>
          <p class="text-xs text-tertiary">{{ t('admin-asset-record-types-catalogue-hint') }}</p>
        </div>
        <ul class="grid gap-3 sm:grid-cols-2">
          <li
            v-for="entry in offered"
            :key="entry.slug"
            data-test="catalogue-entry"
            class="flex flex-col gap-3 p-4 bg-surface border border-default rounded-lg"
          >
            <div class="flex items-center gap-3">
              <span
                class="flex items-center justify-center w-8 h-8 rounded-md bg-surface-alt text-secondary shrink-0"
              >
                <Icon :name="iconOf(entry.icon)" />
              </span>
              <span class="flex-1 min-w-0 font-medium text-primary truncate">
                {{ t(`asset-record-type-${entry.slug}`) }}
              </span>
              <Button
                size="sm"
                variant="secondary"
                icon="add"
                :loading="adding === entry.slug"
                :disabled="adding !== null && adding !== entry.slug"
                :aria-label="t('admin-asset-record-types-add-aria', { name: t(`asset-record-type-${entry.slug}`) })"
                @click="addFromCatalogue(entry)"
              >
                {{ t('admin-asset-record-types-add') }}
              </Button>
            </div>
            <div v-if="entry.outcomes.length > 0" class="flex flex-wrap items-center gap-1">
              <span class="text-xs text-tertiary">{{ t('admin-asset-record-types-catalogue-outcomes') }}</span>
              <span
                v-for="outcome in entry.outcomes"
                :key="outcome.key"
                class="text-xs px-1.5 py-0.5 rounded bg-surface-alt text-secondary"
              >
                {{ catalogueOutcomeName(t, entry.slug, outcome.key) }}
              </span>
            </div>
          </li>
        </ul>
      </section>
    </div>
  </div>
</template>
