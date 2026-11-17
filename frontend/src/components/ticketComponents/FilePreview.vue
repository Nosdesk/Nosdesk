<!-- FilePreview.vue -->
<script setup lang="ts">
import AssetImg from '@/components/common/AssetImg.vue'
import { assetUrl } from '@nosdesk/core/transport'
import DownloadLink from '@/components/common/DownloadLink.vue'
import Icon from '@/components/common/Icon.vue'
import type { IconName } from '@/components/common/icons'
import { button, buttonIconSize } from '@/recipes/button'
import IconButton from '@/components/common/IconButton.vue'
import { computed, ref, onMounted } from 'vue';
import { useFluent } from 'fluent-vue';
import Spinner from '@/components/common/Spinner.vue';

const { $t } = useFluent();

interface Props {
  src: string;
  filename: string;
  author: string;
  timestamp: string;
  showDelete?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  showDelete: false
});

const emit = defineEmits<{
  (e: 'delete'): void;
  (e: 'preview', src: string): void;
}>();

const imagePreview = ref<HTMLImageElement | null>(null);
const isLoadingImage = ref(false);
const thumbnailError = ref<string | null>(null);
const showPreview = ref(false);

const fileExtension = computed(() => {
  const ext = props.filename.split('.').pop()?.toLowerCase() || '';
  return ext;
});

const fileType = computed(() => {
  const ext = fileExtension.value;
  if (ext === 'pdf') return 'pdf';
  if (['doc', 'docx'].includes(ext)) return 'word';
  if (['xls', 'xlsx'].includes(ext)) return 'excel';
  if (['ppt', 'pptx'].includes(ext)) return 'powerpoint';
  if (['txt', 'rtf', 'md'].includes(ext)) return 'text';
  if (['zip', 'rar', '7z'].includes(ext)) return 'archive';
  if (['jpg', 'jpeg', 'png', 'gif', 'apng', 'webp', 'avif', 'jxl'].includes(ext)) return 'image';
  return 'generic';
});

// Add a computed property to detect animated images
const isAnimatedImage = computed(() => {
  return ['gif', 'apng'].includes(fileExtension.value);
});

const fileIcon = computed<IconName>(() => {
  switch (fileType.value) {
    case 'pdf':
    case 'word':
    case 'powerpoint':
      return 'document';
    case 'excel':
      return 'table';
    case 'archive':
      return 'archive';
    default:
      return 'file';
  }
});

const fileColor = computed(() => {
  switch (fileType.value) {
    case 'pdf':
      return 'text-status-error';
    case 'word':
      return 'text-accent';
    case 'excel':
      return 'text-status-success';
    case 'powerpoint':
      return 'text-status-warning';
    case 'archive':
      return 'text-status-warning';
    default:
      return 'text-tertiary';
  }
});

const showOverlay = computed(() => fileType.value === 'image' && !isLoadingImage.value);

const loadImagePreview = async () => {
  if (!props.src || fileType.value !== 'image') return;

  try {
    isLoadingImage.value = true;
    thumbnailError.value = null;

    // Create a new image and wait for it to load
    const img = new Image();
    await new Promise((resolve, reject) => {
      img.onload = resolve;
      img.onerror = () => reject(new Error('Failed to load image'));
      // Resolved for the platform, as the <AssetImg> below is.
      // eslint-disable-next-line no-restricted-syntax
      img.src = assetUrl(props.src);
    });

    imagePreview.value = img;
  } catch (error) {
    console.error('Error loading image preview:', error);
    thumbnailError.value = $t('ticket-media-file-image-error');
  } finally {
    isLoadingImage.value = false;
  }
};

const openPreview = () => {
  if (fileType.value === 'image') {
    showPreview.value = true;
    emit('preview', props.src);
  }
};

// Add new function to extract display name from filename
const getDisplayName = (filename: string): string => {
  if (!filename) return $t('ticket-media-file-fallback');

  // Check if the filename is a UUID pattern followed by an extension
  // Example: 550e8400-e29b-41d4-a716-446655440000.pdf
  const uuidPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.[a-z0-9]+$/i;

  if (uuidPattern.test(filename)) {
    // If it's a UUID, return a friendly name based on the file type
    const ext = fileExtension.value;

    switch (fileType.value) {
      case 'pdf':
        return $t('ticket-media-file-pdf', { ext });
      case 'word':
        return $t('ticket-media-file-word', { ext });
      case 'excel':
        return $t('ticket-media-file-excel', { ext });
      case 'powerpoint':
        return $t('ticket-media-file-powerpoint', { ext });
      case 'image':
        return $t('ticket-media-file-image', { ext });
      case 'archive':
        return $t('ticket-media-file-archive', { ext });
      case 'text':
        return $t('ticket-media-file-text', { ext });
      default:
        return $t('ticket-media-file-generic', { ext });
    }
  }

  // Otherwise, return the original filename
  return filename;
};

// AttachmentPreview shows PDFs itself, so this component only previews
// images and shows an icon for everything else.
onMounted(async () => {
  if (fileType.value === 'image') {
    await loadImagePreview();
  }
});
</script>

<template>
  <div class="flex flex-col gap-2 bg-surface-alt rounded-lg p-3 max-w-[350px]">
    <div class="flex items-center justify-between mb-3">
      <div class="flex items-center gap-2">
        <div class="flex flex-col">
          <span class="text-sm text-primary line-clamp-2">{{ getDisplayName(filename) }}</span>

          <span class="text-xs text-tertiary">{{ timestamp }}</span>
        </div>
      </div>

      <!-- Delete button -->
      <IconButton
        :label="$t('ticket-media-file-delete')"
        icon="trash"
        size="sm"
        v-if="showDelete"
        @click.stop="emit('delete')"
      />
    </div>

    <div class="flex flex-col gap-3 bg-surface/50 rounded-lg p-3">
      <!-- File Preview/Thumbnail -->
      <div class="flex-shrink-0 w-full h-40 flex items-center justify-center rounded-lg overflow-hidden bg-surface-alt relative group">
        <!-- Loading Spinner -->
        <div v-if="isLoadingImage" class="flex items-center justify-center w-full h-full">
          <span class="text-tertiary inline-flex">
            <Spinner size="lg" />
          </span>
        </div>

        <!-- Image Preview -->
        <AssetImg
          v-if="fileType === 'image' && !isLoadingImage"
          :src="src"
          :alt="filename"
          class="w-full h-full object-cover"
          :class="{ 'animated-image': isAnimatedImage }" />

        <!-- Animation badge for GIF/APNG -->
        <div
          v-if="isAnimatedImage"
          class="absolute top-3 left-3 bg-accent/80 px-2 py-1 rounded text-xs text-on-accent font-medium animate-pulse z-20"
        >
          {{ $t('ticket-media-file-animated-badge') }}
        </div>

        <!-- File Icon for other types -->
        <div v-if="fileType !== 'image'" :class="['p-3 rounded-lg', fileColor]">
          <Icon :name="fileIcon" size="2xl" />
        </div>

        <!-- Preview overlay for images -->
        <div
          v-if="showOverlay"
          class="absolute inset-0 bg-surface/60 opacity-0 group-hover:opacity-100 transition-opacity duration-200 flex items-center justify-center cursor-pointer"
          @click="openPreview"
        >
          <Icon name="eye" size="xl" class="text-primary" />
        </div>

        <!-- Delete button for preview -->
        <button
          v-if="showDelete && showOverlay"
          type="button"
          @click.stop="emit('delete')"
          class="absolute top-2 right-2 z-10 p-1.5 bg-surface-alt/80 text-tertiary hover:text-primary hover:bg-surface-hover rounded transition-colors opacity-0 group-hover:opacity-100"
          :title="$t('ticket-media-file-delete')"
          :aria-label="$t('ticket-media-file-delete')"
        >
          <Icon name="trash" />
        </button>
      </div>

      <div class="min-w-0 flex flex-col gap-1">
        <div class="flex items-start gap-2">
          <div class="flex-grow">
            <span class="text-xs text-tertiary uppercase mt-1 block">
              {{ fileExtension }}
            </span>
          </div>
        </div>

        <!-- Download button -->
        <div class="flex items-center gap-2 justify-end mt-2">
          <DownloadLink :href="src" :filename="filename" :class="button({ size: 'sm' })" spinner-size="xs">
            <Icon name="download" :size="buttonIconSize('sm')" />
            <span>{{ $t('ticket-media-file-download') }}</span>
          </DownloadLink>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* Add styles for animated images (GIF, APNG) */
.animated-image {
  border: 2px solid rgb(var(--color-accent) / 0.5);
}

/* Animation for the pulse effect */
@keyframes pulse {
  0%, 100% {
    opacity: 1;
  }
  50% {
    opacity: 0.6;
  }
}

.animate-pulse {
  animation: pulse 2s cubic-bezier(0.4, 0, 0.6, 1) infinite;
}

/* Add a pulsing border animation */
@keyframes pulse-border {
  0%, 100% {
    border-color: rgb(var(--color-accent) / 0.8);
  }
  50% {
    border-color: rgb(var(--color-accent) / 0.4);
  }
}
</style> 