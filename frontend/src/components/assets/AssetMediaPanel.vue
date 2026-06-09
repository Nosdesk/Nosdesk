<script setup lang="ts">
import { computed, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import Button from '@/components/common/Button.vue';
import Icon from '@/components/common/Icon.vue';
import { assetMediaService, assetMediaKeys } from '@/services/assetMediaService';
import { useSyncActions } from '@/composables/useSyncActions';
import type { AssetMedia } from '@/types/asset';

const props = defineProps<{
  assetId: number;
  canEdit?: boolean;
}>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const queryCache = useQueryCache();
const mediaQuery = useQuery({
  key: () => assetMediaKeys.forAsset(props.assetId),
  query: () => assetMediaService.list(props.assetId),
});
const media = computed<AssetMedia[]>(() =>
  Array.isArray(mediaQuery.data.value) ? mediaQuery.data.value : [],
);
// Cold start with no cached payload is the only time there's nothing
// to show; once cached, SWR revalidates silently behind the list.
const isFirstLoad = computed(
  () => mediaQuery.status.value === 'pending' && mediaQuery.data.value === undefined,
);

const uploading = ref(false);
const errorMessage = ref('');
const fileInputRef = ref<HTMLInputElement | null>(null);

function invalidate() {
  return queryCache.invalidateQueries({ key: assetMediaKeys.forAsset(props.assetId) });
}

function openPicker() {
  fileInputRef.value?.click();
}

async function onFileChange(event: Event) {
  const input = event.target as HTMLInputElement;
  const files = Array.from(input.files ?? []);
  input.value = '';
  if (files.length === 0) return;

  uploading.value = true;
  errorMessage.value = '';
  try {
    await assetMediaService.upload(props.assetId, files);
    await invalidate();
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('asset-media-upload-failed');
  } finally {
    uploading.value = false;
  }
}

async function deleteMedia(row: AssetMedia) {
  if (!props.canEdit) return;
  errorMessage.value = '';
  // Optimistically drop the row from the cached list; reconcile with
  // the server on success, restore from the server on failure.
  queryCache.setQueryData(
    assetMediaKeys.forAsset(props.assetId),
    media.value.filter((m) => m.id !== row.id),
  );
  try {
    await assetMediaService.delete(props.assetId, row.id);
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('asset-media-delete-failed');
  } finally {
    await invalidate();
  }
}

useSyncActions(
  (actions) => {
    if (
      actions.some((a) => {
        const data = a.data as { asset_id?: number };
        return data.asset_id === props.assetId;
      })
    ) {
      void invalidate();
    }
  },
  { aggregates: ['asset_media'], debounceMs: 250 },
);
</script>

<template>
  <div class="flex flex-col gap-3">
    <div class="flex items-center justify-between gap-3">
      <p class="text-xs text-tertiary">
        {{ $t('asset-media-description') }}
      </p>
      <Button
        v-if="canEdit"
        size="sm"
        icon="paperclip"
        :loading="uploading"
        @click="openPicker"
      >
        {{ $t('asset-media-add') }}
      </Button>
      <input
        ref="fileInputRef"
        type="file"
        accept="image/*"
        multiple
        class="hidden"
        @change="onFileChange"
      />
    </div>

    <p v-if="errorMessage" class="text-sm text-status-error">{{ errorMessage }}</p>

    <div
      v-if="media.length === 0 && !isFirstLoad"
      class="rounded-lg border border-dashed border-default bg-surface-alt p-4 flex items-start gap-3"
    >
      <Icon name="paperclip" class="text-tertiary flex-shrink-0 mt-0.5" />
      <div class="min-w-0">
        <p class="text-sm font-medium text-primary">{{ $t('asset-media-empty-title') }}</p>
        <p class="text-xs text-tertiary mt-1">{{ $t('asset-media-empty-description') }}</p>
      </div>
    </div>

    <div v-else class="grid grid-cols-2 sm:grid-cols-3 gap-3">
      <div
        v-for="item in media"
        :key="item.id"
        class="group relative rounded-lg overflow-hidden border border-default bg-surface-alt aspect-square"
      >
        <img
          :src="item.url"
          :alt="item.caption || item.name"
          class="w-full h-full object-cover"
          loading="lazy"
        />
        <div class="absolute inset-x-0 bottom-0 bg-black/55 text-white p-2">
          <p class="text-xs font-medium truncate">{{ item.caption || item.name }}</p>
        </div>
        <button
          v-if="canEdit"
          type="button"
          class="absolute top-2 right-2 p-1.5 rounded-md bg-black/60 text-white opacity-0 group-hover:opacity-100 focus:opacity-100 transition-opacity"
          :aria-label="$t('asset-media-delete-aria', { name: item.name })"
          @click="deleteMedia(item)"
        >
          <Icon name="trash" size="xs" />
        </button>
      </div>
    </div>
  </div>
</template>
