<!-- FilePreview.vue -->
<script setup lang="ts">
import { computed, ref, onMounted, nextTick } from 'vue';
import { useFluent } from 'fluent-vue';
import Icon from '@/components/common/Icon.vue';
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

const thumbnailCanvas = ref<HTMLCanvasElement | null>(null);
const imagePreview = ref<HTMLImageElement | null>(null);
const isLoadingThumbnail = ref(false);
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

const fileIcon = computed(() => {
  switch (fileType.value) {
    case 'pdf':
      return `<path fill-rule="evenodd" d="M4 4a2 2 0 012-2h4.586A2 2 0 0112 2.586L15.414 6A2 2 0 0116 7.414V16a2 2 0 01-2 2H6a2 2 0 01-2-2V4zm2 6a1 1 0 011-1h6a1 1 0 110 2H7a1 1 0 01-1-1zm1 3a1 1 0 100 2h6a1 1 0 100-2H7z" clip-rule="evenodd" />`;
    case 'word':
      return `<path fill-rule="evenodd" d="M4 4a2 2 0 012-2h8a2 2 0 012 2v12a2 2 0 01-2 2H6a2 2 0 01-2-2V4zm2 3a1 1 0 011-1h6a1 1 0 110 2H7a1 1 0 01-1-1zm1 3a1 1 0 100 2h6a1 1 0 100-2H7z" clip-rule="evenodd" />`;
    case 'excel':
      return `<path fill-rule="evenodd" d="M6 2a2 2 0 00-2 2v12a2 2 0 002 2h8a2 2 0 002-2V7.414A2 2 0 0015.414 6L12 2.586A2 2 0 0010.586 2H6zm5 6a1 1 0 10-2 0v6a1 1 0 102 0V8zm2-1a1 1 0 011 1v6a1 1 0 11-2 0V8a1 1 0 011-1z" clip-rule="evenodd" />`;
    case 'powerpoint':
      return `<path fill-rule="evenodd" d="M4 4a2 2 0 012-2h8a2 2 0 012 2v12a2 2 0 01-2 2H6a2 2 0 01-2-2V4zm2 3a1 1 0 011-1h6a1 1 0 110 2H7a1 1 0 01-1-1zm1 3a1 1 0 100 2h6a1 1 0 100-2H7z" clip-rule="evenodd" />`;
    case 'archive':
      return `<path fill-rule="evenodd" d="M4 4a2 2 0 012-2h8a2 2 0 012 2v12a2 2 0 01-2 2H6a2 2 0 01-2-2V4zm2 3a1 1 0 011-1h6a1 1 0 110 2H7a1 1 0 01-1-1zm0 3a1 1 0 011-1h6a1 1 0 110 2H7a1 1 0 01-1-1zm0 3a1 1 0 011-1h6a1 1 0 110 2H7a1 1 0 01-1-1z" clip-rule="evenodd" />`;
    default:
      return `<path fill-rule="evenodd" d="M4 4a2 2 0 012-2h4.586A2 2 0 0112 2.586L15.414 6A2 2 0 0116 7.414V16a2 2 0 01-2 2H6a2 2 0 01-2-2V4z" clip-rule="evenodd" />`;
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

const showOverlay = computed(() => {
  if (fileType.value === 'pdf') return !isLoadingThumbnail.value;
  if (fileType.value === 'image') return !isLoadingImage.value;
  return false;
});

const generatePdfThumbnail = async (retryCount = 0) => {
  if (!props.src || fileType.value !== 'pdf') return;
  
  try {
    isLoadingThumbnail.value = true;
    thumbnailError.value = null;

    // Wait for next tick to ensure canvas is mounted
    await nextTick();
    
    if (!thumbnailCanvas.value) {
      // Retry up to 3 times with increasing delays
      if (retryCount < 3) {
        await new Promise(resolve => setTimeout(resolve, 100 * (retryCount + 1)));
        return generatePdfThumbnail(retryCount + 1);
      }
      throw new Error('Canvas element not found');
    }

    // Dynamically import PDF.js only when needed
    const pdfjsLib = await import('pdfjs-dist');
    // Configure the worker source using Vite's asset handling
    pdfjsLib.GlobalWorkerOptions.workerSrc = new URL('pdfjs-dist/build/pdf.worker.mjs', import.meta.url).href;

    // Use loadingTask directly without reactivity. isEvalSupported: false
    // keeps pdf.js off `new Function()` so it doesn't trip the strict CSP.
    const loadingTask = pdfjsLib.getDocument({ url: props.src, isEvalSupported: false });
    // Use shallowRef pattern - don't put PDF document directly in a reactive property
    const pdf = await loadingTask.promise;
    const page = await pdf.getPage(1);

    const viewport = page.getViewport({ scale: 0.5 });
    const canvas = thumbnailCanvas.value;
    const context = canvas.getContext('2d');

    if (!context) {
      throw new Error('Could not get canvas context');
    }

    canvas.height = viewport.height;
    canvas.width = viewport.width;

    await page.render({
      canvasContext: context,
      viewport: viewport
    }).promise;

  } catch (error) {
    console.error('Error generating PDF thumbnail:', error);
    thumbnailError.value = $t('ticket-media-file-thumbnail-error');
  } finally {
    isLoadingThumbnail.value = false;
  }
};

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
      img.src = props.src;
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
  if (fileType.value === 'pdf' || fileType.value === 'image') {
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

onMounted(async () => {
  if (fileType.value === 'pdf') {
    // Wait a bit before attempting to generate the thumbnail
    await nextTick();
    await new Promise(resolve => setTimeout(resolve, 100));
    generatePdfThumbnail();
  } else if (fileType.value === 'image') {
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
      <button
        v-if="showDelete"
        type="button"
        @click.stop="emit('delete')"
        class="p-1.5 text-tertiary hover:text-primary hover:bg-surface-hover rounded transition-colors"
        :title="$t('ticket-media-file-delete')"
      >
        <Icon name="trash" />
      </button>
    </div>

    <div class="flex flex-col gap-3 bg-surface/50 rounded-lg p-3">
      <!-- File Preview/Thumbnail -->
      <div class="flex-shrink-0 w-full h-40 flex items-center justify-center rounded-lg overflow-hidden bg-surface-alt relative group">
        <!-- Loading Spinner -->
        <div v-if="isLoadingThumbnail || isLoadingImage" class="flex items-center justify-center w-full h-full">
          <span class="text-tertiary inline-flex">
            <Spinner size="lg" />
          </span>
        </div>

        <!-- PDF Preview -->
        <canvas 
          v-if="fileType === 'pdf' && !isLoadingThumbnail"
          ref="thumbnailCanvas"
          class="w-full h-full object-contain"
        ></canvas>

        <!-- Image Preview -->
        <img
          v-if="fileType === 'image' && !isLoadingImage"
          ref="imagePreview"
          :src="src"
          :alt="filename"
          class="w-full h-full object-cover"
          :class="{ 'animated-image': isAnimatedImage }"
        >

        <!-- Animation badge for GIF/APNG -->
        <div
          v-if="isAnimatedImage"
          class="absolute top-3 left-3 bg-accent/80 px-2 py-1 rounded text-xs text-on-accent font-medium animate-pulse z-20"
        >
          {{ $t('ticket-media-file-animated-badge') }}
        </div>

        <!-- File Icon for other types -->
        <div v-if="!['pdf', 'image'].includes(fileType)" :class="['p-3 rounded-lg', fileColor]">
          <svg class="w-12 h-12" viewBox="0 0 20 20" fill="currentColor">
            <path v-html="fileIcon"></path>
          </svg>
        </div>

        <!-- Preview Overlay - shared between PDF and Image -->
        <div
          v-if="showOverlay"
          class="absolute inset-0 bg-surface/60 opacity-0 group-hover:opacity-100 transition-opacity duration-200 flex items-center justify-center cursor-pointer"
          @click="openPreview"
        >
          <svg class="w-8 h-8 text-primary" viewBox="0 0 20 20" fill="currentColor">
            <path d="M10 12a2 2 0 100-4 2 2 0 000 4z" />
            <path fill-rule="evenodd" d="M.458 10C1.732 5.943 5.522 3 10 3s8.268 2.943 9.542 7c-1.274 4.057-5.064 7-9.542 7S1.732 14.057.458 10zM14 10a4 4 0 11-8 0 4 4 0 018 0z" clip-rule="evenodd" />
          </svg>
        </div>

        <!-- Delete button for preview -->
        <button
          v-if="showDelete && showOverlay"
          type="button"
          @click.stop="emit('delete')"
          class="absolute top-2 right-2 z-10 p-1.5 bg-surface-alt/80 text-tertiary hover:text-primary hover:bg-surface-hover rounded transition-colors opacity-0 group-hover:opacity-100"
          :title="$t('ticket-media-file-delete')"
        >
          <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
          </svg>
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
          <a
            :href="src"
            target="_blank"
            class="px-3 py-1.5 bg-accent text-on-accent text-sm rounded hover:opacity-90 transition-colors flex items-center gap-2"
            :download="filename"
          >
            <Icon name="download" />
            {{ $t('ticket-media-file-download') }}
          </a>
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