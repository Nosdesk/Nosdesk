<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import FormInput from '@/components/common/FormInput.vue';
import Icon from '@/components/common/Icon.vue';
import Modal from '@/components/Modal.vue';
import { assetMediaService, assetMediaKeys } from '@nosdesk/core/services/assetMediaService';
import { useSyncActions } from '@/composables/useSyncActions';
import type { AssetMedia } from '@nosdesk/core/types/asset';

const props = defineProps<{
  assetId: number;
  canEdit?: boolean;
  /** Render a tighter 2-column grid for narrow placements (e.g. the
   *  asset detail rail), instead of widening to 3 columns on larger
   *  viewports. */
  compact?: boolean;
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
const isFirstLoad = computed(
  () => mediaQuery.status.value === 'pending' && mediaQuery.data.value === undefined,
);

const uploading = ref(false);
const errorMessage = ref('');
const fileInputRef = ref<HTMLInputElement | null>(null);

const lightboxOpen = ref(false);
const lightboxIndex = ref(0);
const lightboxItem = computed(() => media.value[lightboxIndex.value] ?? null);

const editingCaptionId = ref<number | null>(null);
const captionDraft = ref('');
const captionSaving = ref(false);

const dragMediaId = ref<number | null>(null);
const reordering = ref(false);

function invalidate() {
  return queryCache.invalidateQueries({ key: assetMediaKeys.forAsset(props.assetId) });
}

function setMediaCache(rows: AssetMedia[]) {
  queryCache.setQueryData(assetMediaKeys.forAsset(props.assetId), rows);
}

function gridSrc(item: AssetMedia): string {
  return item.thumbnail_url || item.url;
}

function openPicker() {
  fileInputRef.value?.click();
}

// The add control lives in the host card header (AssetView's
// SectionCard #headerActions), so expose the trigger + upload state.
defineExpose({ openPicker, uploading });

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
  setMediaCache(media.value.filter((m) => m.id !== row.id));
  try {
    await assetMediaService.delete(props.assetId, row.id);
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('asset-media-delete-failed');
  } finally {
    await invalidate();
  }
}

function openLightbox(index: number) {
  lightboxIndex.value = index;
  lightboxOpen.value = true;
}

function closeLightbox() {
  lightboxOpen.value = false;
}

function lightboxPrev() {
  if (media.value.length === 0) return;
  lightboxIndex.value = (lightboxIndex.value - 1 + media.value.length) % media.value.length;
}

function lightboxNext() {
  if (media.value.length === 0) return;
  lightboxIndex.value = (lightboxIndex.value + 1) % media.value.length;
}

function onLightboxKeydown(event: KeyboardEvent) {
  if (!lightboxOpen.value) return;
  if (event.key === 'ArrowLeft') {
    event.preventDefault();
    lightboxPrev();
  } else if (event.key === 'ArrowRight') {
    event.preventDefault();
    lightboxNext();
  }
}

function startCaptionEdit(item: AssetMedia) {
  if (!props.canEdit) return;
  editingCaptionId.value = item.id;
  captionDraft.value = item.caption ?? '';
}

function cancelCaptionEdit() {
  editingCaptionId.value = null;
  captionDraft.value = '';
}

async function saveCaption(item: AssetMedia) {
  if (!props.canEdit || captionSaving.value) return;
  const next = captionDraft.value.trim() || null;
  editingCaptionId.value = null;
  captionSaving.value = true;
  errorMessage.value = '';
  setMediaCache(
    media.value.map((m) => (m.id === item.id ? { ...m, caption: next } : m)),
  );
  try {
    await assetMediaService.update(props.assetId, item.id, { caption: next });
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('asset-media-caption-failed');
  } finally {
    captionSaving.value = false;
    await invalidate();
  }
}

function onDragStart(item: AssetMedia, event: DragEvent) {
  if (!props.canEdit || reordering.value) return;
  dragMediaId.value = item.id;
  event.dataTransfer?.setData('text/plain', String(item.id));
  if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move';
}

function onDragOver(event: DragEvent) {
  if (!props.canEdit) return;
  event.preventDefault();
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
}

async function onDrop(target: AssetMedia) {
  if (!props.canEdit || dragMediaId.value == null || reordering.value) return;
  const fromIdx = media.value.findIndex((m) => m.id === dragMediaId.value);
  const toIdx = media.value.findIndex((m) => m.id === target.id);
  dragMediaId.value = null;
  if (fromIdx < 0 || toIdx < 0 || fromIdx === toIdx) return;

  const reordered = [...media.value];
  const [moved] = reordered.splice(fromIdx, 1);
  reordered.splice(toIdx, 0, moved);
  const before = media.value;
  const withOrder = reordered.map((m, index) => ({ ...m, sort_order: index }));
  setMediaCache(withOrder);

  reordering.value = true;
  errorMessage.value = '';
  try {
    const updates = withOrder.filter((item, index) => {
      const prev = before.find((m) => m.id === item.id);
      return prev?.sort_order !== index;
    });
    await Promise.all(
      updates.map((item) => {
        const index = withOrder.findIndex((m) => m.id === item.id);
        return assetMediaService.update(props.assetId, item.id, { sort_order: index });
      }),
    );
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('asset-media-reorder-failed');
  } finally {
    reordering.value = false;
    await invalidate();
  }
}

onMounted(() => window.addEventListener('keydown', onLightboxKeydown));
onUnmounted(() => window.removeEventListener('keydown', onLightboxKeydown));

watch(lightboxOpen, (open) => {
  if (!open) return;
  const idx = media.value.findIndex((m) => m.id === lightboxItem.value?.id);
  if (idx >= 0) lightboxIndex.value = idx;
});

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
    <!-- The add control lives in the SectionCard header (see AssetView);
         this panel exposes openPicker() for it to call. -->
    <input
      ref="fileInputRef"
      type="file"
      accept="image/*"
      multiple
      class="hidden"
      @change="onFileChange"
    />

    <p v-if="errorMessage" class="text-sm text-status-error">{{ errorMessage }}</p>

    <div v-if="uploading" class="flex justify-center py-1">
      <span class="inline-block animate-spin rounded-full h-4 w-4 border-b-2 border-accent" />
    </div>

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

    <div v-else class="grid gap-2" :class="compact ? 'grid-cols-3' : 'grid-cols-2 sm:grid-cols-3'">
      <div
        v-for="(item, index) in media"
        :key="item.id"
        class="group relative rounded-lg overflow-hidden border border-default bg-surface-alt aspect-square"
        :class="canEdit && !reordering ? 'cursor-grab active:cursor-grabbing' : ''"
        :draggable="canEdit && !reordering"
        @dragstart="onDragStart(item, $event)"
        @dragover="onDragOver"
        @drop="onDrop(item)"
      >
        <button
          type="button"
          class="block w-full h-full focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/50"
          @click="openLightbox(index)"
        >
          <img
            :src="gridSrc(item)"
            :alt="item.caption || item.name"
            class="w-full h-full object-cover"
            loading="lazy"
            draggable="false"
          />
        </button>

        <div
          v-if="editingCaptionId === item.id"
          class="absolute inset-x-0 bottom-0 bg-black/75 p-2 flex flex-col gap-1.5"
          @click.stop
        >
          <FormInput
            v-model="captionDraft"
            size="sm"
            :placeholder="$t('asset-media-caption-placeholder')"
            @keyup.enter="saveCaption(item)"
            @keyup.escape="cancelCaptionEdit"
          />
          <div class="flex items-center gap-1.5 justify-end">
            <button
              type="button"
              class="text-[10px] px-2 py-0.5 rounded text-white/80 hover:text-white"
              @click="cancelCaptionEdit"
            >
              {{ $t('asset-media-caption-cancel') }}
            </button>
            <button
              type="button"
              class="text-[10px] px-2 py-0.5 rounded bg-accent text-on-accent"
              :disabled="captionSaving"
              @click="saveCaption(item)"
            >
              {{ $t('asset-media-caption-save') }}
            </button>
          </div>
        </div>
        <div
          v-else
          class="absolute inset-x-0 bottom-0 bg-black/55 text-white p-2 pointer-events-none"
        >
          <p class="text-xs font-medium truncate">{{ item.caption || item.name }}</p>
        </div>

        <div
          v-if="canEdit"
          class="absolute top-2 left-2 flex items-center gap-1 opacity-0 group-hover:opacity-100 focus-within:opacity-100 transition-opacity"
        >
          <button
            type="button"
            class="p-1.5 rounded-md bg-black/60 text-white"
            :aria-label="$t('asset-media-caption-edit-aria', { name: item.name })"
            @click.stop="startCaptionEdit(item)"
          >
            <Icon name="rename" size="xs" />
          </button>
        </div>

        <button
          v-if="canEdit"
          type="button"
          class="absolute top-2 right-2 p-1.5 rounded-md bg-black/60 text-white opacity-0 group-hover:opacity-100 focus:opacity-100 transition-opacity"
          :aria-label="$t('asset-media-delete-aria', { name: item.name })"
          @click.stop="deleteMedia(item)"
        >
          <Icon name="trash" size="xs" />
        </button>
      </div>
    </div>

    <Modal
      :show="lightboxOpen"
      :title="lightboxItem?.caption || lightboxItem?.name || $t('asset-media-heading')"
      size="lg"
      :remove-padding="true"
      @close="closeLightbox"
    >
      <div v-if="lightboxItem" class="flex flex-col">
        <div class="relative bg-black flex items-center justify-center min-h-[50vh] max-h-[70vh]">
          <img
            :src="lightboxItem.url"
            :alt="lightboxItem.caption || lightboxItem.name"
            class="max-w-full max-h-[70vh] object-contain"
          />
          <button
            v-if="media.length > 1"
            type="button"
            class="absolute left-2 top-1/2 -translate-y-1/2 p-2 rounded-full bg-black/60 text-white hover:bg-black/80"
            :aria-label="$t('asset-media-lightbox-previous')"
            @click="lightboxPrev"
          >
            <Icon name="chevronLeft" />
          </button>
          <button
            v-if="media.length > 1"
            type="button"
            class="absolute right-2 top-1/2 -translate-y-1/2 p-2 rounded-full bg-black/60 text-white hover:bg-black/80"
            :aria-label="$t('asset-media-lightbox-next')"
            @click="lightboxNext"
          >
            <Icon name="chevronRight" />
          </button>
        </div>
        <div class="px-4 py-3 border-t border-default flex flex-col gap-1">
          <p class="text-sm font-medium text-primary">
            {{ lightboxItem.caption || lightboxItem.name }}
          </p>
          <p v-if="lightboxItem.caption" class="text-xs text-tertiary">{{ lightboxItem.name }}</p>
        </div>
      </div>
    </Modal>
  </div>
</template>
