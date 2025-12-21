<script setup lang="ts">
import { computed, ref, shallowRef, onMounted, onBeforeUnmount, nextTick } from 'vue';
import AudioPlayer from "@/components/ticketComponents/AudioPlayer.vue";
import VideoPlayer from "@/components/ticketComponents/VideoPlayer.vue";
import FilePreview from "@/components/ticketComponents/FilePreview.vue";
import PDFViewer from "@/components/ticketComponents/PDFViewer.vue";
import Modal from "@/components/Modal.vue";
import UserAvatar from "@/components/UserAvatar.vue";
import { convertToAuthenticatedPath } from '@/services/fileService';

interface Props {
  attachment: { id?: number; url: string; name: string; comment_id?: number };
  author: string;
  timestamp: string;
  isNew?: boolean;
  showDelete?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  isNew: false,
  showDelete: false
});

const emit = defineEmits<{
  (e: 'delete'): void;
  (e: 'submit'): void;
  (e: 'preview', src: string): void;
}>();

// Convert attachment URL to authenticated URL for secure file access
const authenticatedUrl = computed(() => {
  return convertToAuthenticatedPath(props.attachment.url)
})

const showPreviewModal = ref(false);
const previewImageSrc = ref('');
const showPlaceholder = ref(false);

const log = (event: string, details?: any) => {
  if (import.meta.env.DEV) {
    console.log(`[AttachmentPreview] ${event}`, details || '');
  }
};

// Add a more detailed debug logging function
const debugLog = (event: string, details?: any) => {
  if (import.meta.env.DEV) {
    console.log(`[AttachmentPreview:DEBUG] ${event}`, details || '', new Date().toISOString());
  }
};

const isVideoFile = (filename: string): boolean => {
  try {
    const videoExtensions = ['.mp4', '.mov', '.webm', '.avi', '.mkv'];
    return videoExtensions.some(ext => filename.toLowerCase().endsWith(ext));
  } catch (error) {
    console.error('Error checking if file is a video:', error);
    return false;
  }
};

const isAudioFile = (filename: string): boolean => {
  try {
    const audioExtensions = ['.mp3', '.wav', '.ogg', '.m4a', '.webm'];
    return audioExtensions.some(ext => filename.toLowerCase().endsWith(ext)) || 
           (typeof filename === 'string' && filename.toLowerCase().includes('voice note'));
  } catch (error) {
    console.error('Error checking if file is audio:', error);
    return false;
  }
};

const isImageFile = (filename: string): boolean => {
  try {
    const imageExtensions = ['.jpg', '.jpeg', '.png', '.gif', '.apng', '.webp', '.svg', '.avif', '.jxl'];
    return imageExtensions.some(ext => filename.toLowerCase().endsWith(ext));
  } catch (error) {
    console.error('Error checking if file is an image:', error);
    return false;
  }
};

const isPdfFile = (filename: string): boolean => {
  try {
    if (!filename) return false;
    return filename.toLowerCase().endsWith('.pdf');
  } catch (error) {
    console.error('Error checking if file is PDF:', error);
    return false;
  }
};

// Check if file is an animated image format
const isAnimatedImage = (filename: string): boolean => {
  try {
    if (!filename) return false;
    const animatedExtensions = ['.gif', '.apng'];
    return animatedExtensions.some(ext => filename.toLowerCase().endsWith(ext));
  } catch (error) {
    console.error('Error checking if file is an animated image:', error);
    return false;
  }
};

const attachmentType = computed(() => {
  // Safety check for null/undefined attachment name
  if (!props.attachment?.name) {
    return 'file';
  }
  
  if (isAudioFile(props.attachment.name)) return 'audio';
  if (isVideoFile(props.attachment.name)) return 'video';
  if (isImageFile(props.attachment.name)) return 'image';
  if (isPdfFile(props.attachment.name)) return 'pdf';
  return 'file';
});

const openImagePreview = async (src: string) => {
  // For PDF files, always use the original authenticated URL
  if (isPdfFile(props.attachment.name)) {
    previewImageSrc.value = src;
    showPreviewModal.value = true;
    emit('preview', src);
    return;
  }

  // For all image formats, use the original source
  previewImageSrc.value = src;
  showPreviewModal.value = true;
  emit('preview', previewImageSrc.value);
};

const closeImagePreview = () => {
  showPreviewModal.value = false;
};

// Load PDF thumbnails on component mount
onMounted(() => {
  debugLog('Component onMounted hook started');

  if (isPdfFile(props.attachment.name)) {
    // Generate PDF thumbnail
    generatePdfThumbnail(props.attachment.url);
  }
});

// Clean up object URLs when component is unmounted
onBeforeUnmount(() => {
  debugLog('Component unmounting, cleaning up resources');

  if (pdfThumbnailSrc.value) {
    URL.revokeObjectURL(pdfThumbnailSrc.value);
    debugLog('Revoked PDF thumbnail URL');
  }
});

// Add a function to check if the browser supports a specific image format
const checkBrowserSupport = () => {
  const supportedFormats = {
    avif: false,
    jxl: false
  };
  
  // Create test images to check format support
  const testAvif = new Image();
  testAvif.onload = () => { supportedFormats.avif = true; };
  testAvif.onerror = () => { supportedFormats.avif = false; };
  testAvif.src = 'data:image/avif;base64,AAAAIGZ0eXBhdmlmAAAAAGF2aWZtaWYxbWlhZk1BMUIAAADybWV0YQAAAAAAAAAoaGRscgAAAAAAAAAAcGljdAAAAAAAAAAAAAAAAGxpYmF2aWYAAAAADnBpdG0AAAAAAAEAAAAeaWxvYwAAAABEAAABAAEAAAABAAABGgAAAB0AAAAoaWluZgAAAAAAAQAAABppbmZlAgAAAAABAABhdjAxQ29sb3IAAAAAamlwcnAAAABLaXBjbwAAABRpc3BlAAAAAAAAAAIAAAACAAAAEHBpeGkAAAAAAwgICAAAAAxhdjFDgQ0MAAAAABNjb2xybmNseAACAAIAAYAAAAAXaXBtYQAAAAAAAAABAAEEAQKDBAAAACVtZGF0EgAKCBgANogQEAwgMg8f8D///8WfhwB8+ErK42A=';
  
  const testJxl = new Image();
  testJxl.onload = () => { supportedFormats.jxl = true; };
  testJxl.onerror = () => { supportedFormats.jxl = false; };
  testJxl.src = 'data:image/jxl;base64,/woIELASCAgQAFwASxLFgkWAHL0xqnCBCV0qDp901Te/5QM=';
  
  return supportedFormats;
};

// Store browser format support results
const browserSupport = ref(checkBrowserSupport());

// Add a function to detect formats that may need conversion based on browser support
const needsConversion = (filename: string): boolean => {
  if (!filename) return false;

  // Check if browser supports AVIF
  if (filename.toLowerCase().endsWith('.avif') && !browserSupport.value.avif) return true;

  // Check if browser supports JXL
  if (filename.toLowerCase().endsWith('.jxl') && !browserSupport.value.jxl) return true;

  return false;
};

// Add a function to generate friendly display names for files
const getDisplayName = (filename: string): string => {
  if (!filename) return 'File';
  
  // Check if the filename is a UUID pattern followed by an extension
  // Example: 550e8400-e29b-41d4-a716-446655440000.pdf
  const uuidPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.[a-z0-9]+$/i;
  
  if (uuidPattern.test(filename)) {
    // If it's a UUID, return a friendly name based on the file type
    const ext = filename.split('.').pop()?.toLowerCase() || '';
    
    if (isPdfFile(filename)) return `PDF Document.${ext}`;
    if (isVideoFile(filename)) return `Video.${ext}`;
    if (isAudioFile(filename)) return `Audio.${ext}`;
    if (isImageFile(filename)) return `Image.${ext}`;
    return `File.${ext}`;
  }
  
  // Otherwise, return the original filename
  return filename;
};

const pdfThumbnailCanvas = ref<HTMLCanvasElement | null>(null);
const isPdfThumbnailLoading = ref<boolean>(false);
const pdfThumbnailSrc = ref<string | null>(null); // Separate variable for PDF thumbnails

// Function to generate PDF thumbnails for the grid view
const generatePdfThumbnail = async (url: string) => {
  isPdfThumbnailLoading.value = true;
  showPlaceholder.value = true;
  
  try {
    const pdfjsLib = await import('pdfjs-dist');
    pdfjsLib.GlobalWorkerOptions.workerSrc = '/pdfjs/pdf.worker.min.js';
    
    const response = await fetch(authenticatedUrl.value);
    const arrayBuffer = await response.arrayBuffer();
    const pdf = await pdfjsLib.getDocument({ data: new Uint8Array(arrayBuffer) }).promise;
    const page = await pdf.getPage(1);
    
    const scale = 2;
    const viewport = page.getViewport({ scale });
    
    const canvas = document.createElement('canvas');
    const context = canvas.getContext('2d');
    if (!context) throw new Error('Could not get canvas context');
    
    canvas.height = viewport.height;
    canvas.width = viewport.width;
    
    await page.render({
      canvasContext: context,
      viewport: viewport
    }).promise;
    
    canvas.toBlob((blob) => {
      if (blob) {
        // Store PDF thumbnail separately - don't overwrite previewImageSrc
        pdfThumbnailSrc.value = URL.createObjectURL(blob);
        showPlaceholder.value = false;
      }
      isPdfThumbnailLoading.value = false;
    }, 'image/png', 0.8);
    
  } catch (error) {
    console.error('Error generating PDF thumbnail:', error);
    isPdfThumbnailLoading.value = false;
    showPlaceholder.value = false;
  }
};
</script>

<template>
  <div :class="[
    'flex flex-col gap-2',
    attachmentType === 'audio' ? 'w-full' : '',
    attachmentType === 'video' ? 'w-full' : '',
    attachmentType === 'image' ? 'max-w-[250px]' : '',
    attachmentType === 'pdf' ? 'max-w-[250px]' : ''
  ]">
    <!-- Audio/Video header -->
    <template v-if="attachmentType === 'audio' || attachmentType === 'video'">
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-2">
          <template v-if="attachmentType === 'audio'">
            <div class="flex flex-col">
              <span class="text-sm text-primary">{{ attachment.name }}</span>
            </div>
          </template>
          <template v-if="attachmentType === 'video'">
            <svg class="w-5 h-5 text-tertiary" viewBox="0 0 20 20" fill="currentColor">
              <path d="M2 6a2 2 0 012-2h6a2 2 0 012 2v8a2 2 0 01-2 2H4a2 2 0 01-2-2V6zM14.553 7.106A1 1 0 0014 8v4a1 1 0 00.553.894l2 1A1 1 0 0018 13V7a1 1 0 00-1.447-.894l-2 1z" />
            </svg>
            <span class="text-sm text-secondary">{{ attachment.name }}</span>
          </template>
        </div>
        <div class="flex items-center gap-2">
          <!-- Download button -->
          <a
            :href="authenticatedUrl"
            target="_blank"
            :download="attachment.name"
            class="p-1.5 text-tertiary hover:text-primary hover:bg-surface-hover rounded transition-colors"
            title="Download attachment"
            @click.stop
          >
            <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
              <path fill-rule="evenodd" d="M3 17a1 1 0 011-1h12a1 1 0 110 2H4a1 1 0 01-1-1zm3.293-7.707a1 1 0 011.414 0L9 10.586V3a1 1 0 112 0v7.586l1.293-1.293a1 1 0 111.414 1.414l-3 3a1 1 0 01-1.414 0l-3-3a1 1 0 010-1.414z" clip-rule="evenodd" />
            </svg>
          </a>
          <!-- Delete button -->
          <button
            v-if="showDelete"
            type="button"
            @click.stop="emit('delete')"
            class="p-1.5 text-tertiary hover:text-primary hover:bg-surface-hover rounded transition-colors"
            :title="'Delete ' + attachmentType"
          >
            <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
              <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
            </svg>
          </button>
        </div>
      </div>
    </template>

    <!-- Content -->
    <template v-if="attachmentType === 'audio'">
      <AudioPlayer :src="authenticatedUrl" />
    </template>
    <template v-else-if="attachmentType === 'video'">
      <VideoPlayer
        :src="authenticatedUrl"
        class="w-full h-64"
        :show-delete="showDelete"
        @delete="emit('delete')"
      />
    </template>
    <template v-else-if="attachmentType === 'image'">
      <!-- Image container -->
      <div class="relative group w-full min-w-42 h-58 rounded-lg overflow-hidden bg-surface-alt">
        <button
          v-if="showDelete"
          type="button"
          @click.stop="emit('delete')"
          class="absolute top-2 right-2 z-30 p-1.5 bg-surface-alt/80 text-tertiary hover:text-primary hover:bg-surface-hover rounded transition-colors"
          title="Delete image"
        >
          <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
          </svg>
        </button>

        <!-- Fallback for unsupported formats -->
        <div
          v-if="needsConversion(attachment.name)"
          class="w-full h-full flex items-center justify-center bg-surface-alt p-4"
        >
          <div class="flex flex-col items-center text-center gap-2">
            <svg class="w-12 h-12 mx-auto text-tertiary mb-3" viewBox="0 0 24 24" fill="none" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z" />
            </svg>
            <p class="text-sm text-secondary">This image format is not supported by your browser</p>
          </div>
        </div>
        <!-- Regular image display -->
        <img
          v-else
          :src="authenticatedUrl"
          :alt="attachment.name"
          class="w-full h-full object-cover bg-transparent"
          :class="{
            'animated-preview': isAnimatedImage(attachment.name)
          }"
        >

        <!-- Preview hover overlay -->
        <div
          class="absolute inset-0 bg-surface/30 opacity-0 group-hover:opacity-100 transition-opacity duration-200 flex items-center justify-center cursor-pointer z-25"
          @click.stop="openImagePreview(authenticatedUrl)"
        >
          <svg class="w-8 h-8 text-primary" viewBox="0 0 20 20" fill="currentColor">
            <path d="M10 12a2 2 0 100-4 2 2 0 000 4z" />
            <path fill-rule="evenodd" d="M.458 10C1.732 5.943 5.522 3 10 3s8.268 2.943 9.542 7c-1.274 4.057-5.064 7-9.542 7S1.732 14.057.458 10zM14 10a4 4 0 11-8 0 4 4 0 018 0z" clip-rule="evenodd" />
          </svg>

          <!-- Animation indicator for GIF and APNG -->
          <div
            v-if="isAnimatedImage(attachment.name)"
            class="absolute top-2 left-2 bg-accent/80 px-2 py-1 rounded text-xs text-white font-medium animate-pulse"
          >
            ANIMATED
          </div>
        </div>
        <div
          class="absolute top-0 left-0 p-2 bg-gradient-to-b from-surface/80 to-transparent w-full opacity-0 group-hover:opacity-100 transition-opacity duration-200 z-25"
        >
          <span class="text-sm text-primary font-medium truncate block">{{ getDisplayName(attachment.name) }}</span>
        </div>
        <!-- Download button for images -->
        <div
          class="absolute bottom-2 right-2 opacity-0 group-hover:opacity-100 transition-opacity duration-200 z-25"
        >
          <a
            :href="authenticatedUrl"
            target="_blank"
            :download="attachment.name"
            class="flex items-center gap-1 p-2 bg-surface-alt/80 text-tertiary hover:text-primary hover:bg-surface-hover rounded transition-colors"
            title="Download image"
            @click.stop
          >
            <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
              <path fill-rule="evenodd" d="M3 17a1 1 0 011-1h12a1 1 0 110 2H4a1 1 0 01-1-1zm3.293-7.707a1 1 0 011.414 0L9 10.586V3a1 1 0 112 0v7.586l1.293-1.293a1 1 0 111.414 1.414l-3 3a1 1 0 01-1.414 0l-3-3a1 1 0 010-1.414z" clip-rule="evenodd" />
            </svg>
          </a>
        </div>
      </div>
    </template>
    <template v-else-if="attachmentType === 'pdf'">
      <div class="relative group w-full min-w-42 h-58 rounded-lg overflow-hidden bg-surface-alt">
        <!-- Delete button -->
        <button
          v-if="showDelete"
          type="button"
          @click.stop="emit('delete')"
          class="absolute top-2 right-2 z-30 p-1.5 bg-surface-alt/80 text-tertiary hover:text-primary hover:bg-surface-hover rounded transition-colors"
          title="Delete PDF"
        >
          <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
          </svg>
        </button>
        
        <!-- PDF Loading state -->
        <div
          v-if="isPdfThumbnailLoading"
          class="absolute inset-0 flex flex-col items-center justify-center bg-surface/90 z-20"
        >
          <svg class="animate-spin h-8 w-8 text-status-error mb-2" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
          <span class="text-sm text-primary font-medium">Loading PDF</span>
        </div>

        <!-- PDF Thumbnail Display -->
        <div v-if="pdfThumbnailSrc" class="w-full h-full flex items-center justify-center">
          <img
            :src="pdfThumbnailSrc"
            :alt="attachment.name"
            class="w-full h-full object-contain"
          />
        </div>

        <!-- Fallback PDF icon when no thumbnail -->
        <div v-else-if="!isPdfThumbnailLoading" class="w-full h-full flex items-center justify-center bg-surface-hover">
          <svg class="w-16 h-16 text-status-error" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M4 4a2 2 0 012-2h4.586A2 2 0 0112 2.586L15.414 6A2 2 0 0116 7.414V16a2 2 0 01-2 2H6a2 2 0 01-2-2V4zm2 6a1 1 0 011-1h6a1 1 0 110 2H7a1 1 0 01-1-1zm1 3a1 1 0 100 2h6a1 1 0 100-2H7z" clip-rule="evenodd" />
          </svg>
        </div>

        <!-- PDF overlay with icon -->
        <div
          class="absolute bottom-0 left-0 right-0 p-3 bg-gradient-to-t from-surface/90 to-transparent"
        >
          <div class="flex items-center gap-2">
            <svg class="w-5 h-5 text-status-error" viewBox="0 0 20 20" fill="currentColor">
              <path fill-rule="evenodd" d="M4 4a2 2 0 012-2h4.586A2 2 0 0112 2.586L15.414 6A2 2 0 0116 7.414V16a2 2 0 01-2 2H6a2 2 0 01-2-2V4zm2 6a1 1 0 011-1h6a1 1 0 110 2H7a1 1 0 01-1-1zm1 3a1 1 0 100 2h6a1 1 0 100-2H7z" clip-rule="evenodd" />
            </svg>
            <span class="text-sm text-primary font-medium truncate">{{ getDisplayName(attachment.name) }}</span>
          </div>
        </div>

        <!-- Preview hover overlay -->
        <div
          class="absolute inset-0 bg-surface/30 opacity-0 group-hover:opacity-100 transition-opacity duration-200 flex items-center justify-center cursor-pointer z-25"
          @click.stop="openImagePreview(authenticatedUrl)"
        >
          <svg class="w-12 h-12 text-primary" viewBox="0 0 20 20" fill="currentColor">
            <path d="M10 12a2 2 0 100-4 2 2 0 000 4z" />
            <path fill-rule="evenodd" d="M.458 10C1.732 5.943 5.522 3 10 3s8.268 2.943 9.542 7c-1.274 4.057-5.064 7-9.542 7S1.732 14.057.458 10zM14 10a4 4 0 11-8 0 4 4 0 018 0z" clip-rule="evenodd" />
          </svg>
        </div>
        
        <!-- Download button for PDF -->
        <div 
          class="absolute bottom-2 right-2 opacity-0 group-hover:opacity-100 transition-opacity duration-200 z-25"
        >
          <a
            :href="authenticatedUrl"
            target="_blank"
            :download="attachment.name"
            class="flex items-center gap-1 p-2 bg-accent text-white hover:opacity-90 rounded transition-colors"
            title="Download PDF"
            @click.stop
          >
            <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
              <path fill-rule="evenodd" d="M3 17a1 1 0 011-1h12a1 1 0 110 2H4a1 1 0 01-1-1zm3.293-7.707a1 1 0 011.414 0L9 10.586V3a1 1 0 112 0v7.586l1.293-1.293a1 1 0 111.414 1.414l-3 3a1 1 0 01-1.414 0l-3-3a1 1 0 010-1.414z" clip-rule="evenodd" />
            </svg>
            <span class="text-xs font-medium">PDF</span>
          </a>
        </div>
      </div>
    </template>
    <template v-else>
      <FilePreview
        :src="authenticatedUrl"
        :filename="attachment.name"
        :author="author"
        :timestamp="timestamp"
        :show-delete="showDelete"
        @delete="emit('delete')"
        @preview="(src) => emit('preview', src)"
      />
    </template>

    <!-- Submit controls for new attachments -->
    <div v-if="isNew && attachmentType === 'video'" class="flex justify-end gap-2 mt-2">
      <button
        type="button"
        @click.stop="emit('delete')"
        class="px-3 py-1.5 text-secondary hover:text-primary transition-colors"
      >
        Cancel
      </button>
      <button
        type="button"
        @click.stop="emit('submit')"
        class="px-3 py-1.5 bg-accent text-white text-sm rounded hover:opacity-90 transition-colors"
      >
        Submit Video
      </button>
    </div>
    
    <!-- Image Preview Modal -->
    <Modal 
      :show="showPreviewModal" 
      :title="
        isPdfFile(attachment.name) ? 'PDF Document' : 
        isAnimatedImage(attachment.name) ? 'Animated Image Preview' : 
        'Image Preview'
      "
      :contentClass="isPdfFile(attachment.name) ? 'modal-content-pdf-fullscreen' : ''"
      :headerClass="isPdfFile(attachment.name) ? 'pdf-modal-header' : ''"
      :removePadding="isPdfFile(attachment.name)"
      @close="closeImagePreview"
    >
      <div class="flex flex-col items-center gap-1" :class="{ 'h-full': isPdfFile(attachment.name) }">
        <!-- Content preview with transition -->
        <transition name="fade" mode="out-in" appear>
          <!-- PDF Viewer Component -->
          <PDFViewer 
            v-if="isPdfFile(attachment.name)" 
            :src="previewImageSrc" 
            :filename="attachment.name"
            :initialPage="1"
            class="w-full h-full"
            @error="(error) => console.error('PDF Viewer error:', error)"
          />
          
          <!-- Image Preview (Non-PDF) -->
          <div v-else class="relative">
            <img 
              :src="previewImageSrc" 
              :alt="attachment.name" 
              class="max-w-full max-h-[70vh] object-contain bg-transparent"
              :class="{
                'animated-preview-modal': isAnimatedImage(attachment.name)
              }"
            />
            
            <!-- Animation indicator for GIF/APNG -->
            <div
              v-if="isAnimatedImage(attachment.name)"
              class="absolute top-2 right-2 bg-accent px-3 py-1 rounded-full text-xs text-white font-medium animate-pulse"
            >
              ANIMATED
            </div>
          </div>
        </transition>
        
        <div v-if="!isPdfFile(attachment.name)" class="mt-4 text-center text-sm text-secondary">
          {{ getDisplayName(attachment.name) }}
        </div>

        <!-- Show download buttons only for non-PDF files since PDFViewer has its own -->
        <div v-if="!isPdfFile(attachment.name)" class="mt-4 flex gap-3">
          <!-- For animated images, add a special title -->
          <a
            v-if="isAnimatedImage(attachment.name)"
            :href="previewImageSrc"
            target="_blank"
            :download="attachment.name"
            class="px-4 py-2 bg-accent text-white text-sm rounded hover:opacity-90 transition-colors flex items-center gap-2"
          >
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4"></path>
            </svg>
            Download animated image
          </a>

          <!-- For regular images, show the standard download button -->
          <a
            v-else
            :href="previewImageSrc"
            target="_blank"
            :download="attachment.name"
            class="px-4 py-2 bg-surface-alt text-primary text-sm rounded hover:bg-surface-hover transition-colors flex items-center gap-2"
          >
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4"></path>
            </svg>
            Download image
          </a>
        </div>
      </div>
    </Modal>
  </div>
</template>

<style scoped>
/* Add transition styles */
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.3s ease, transform 0.3s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
  transform: scale(0.95);
}

.fade-enter-to,
.fade-leave-from {
  opacity: 1;
  transform: scale(1);
}

/* Optional: Add this class to the modal to remove any background */
:deep(.modal-content) {
  background-color: rgba(15, 23, 42, 0.95); /* Slightly transparent dark background */
}

/* Add a pulse animation for the loading spinner */
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
    border-color: rgba(59, 130, 246, 0.8);
  }
  50% {
    border-color: rgba(96, 165, 250, 0.4);
  }
}

/* Add styles for animated images (GIF, APNG) */
.animated-image {
  border: 2px solid transparent;
  animation: pulse-border 2s ease-in-out infinite;
  border-color: rgba(99, 102, 241, 0.5); /* Indigo color to indicate animation */
}

/* Animation indicator animation */
@keyframes flash {
  0%, 100% {
    background-color: rgba(99, 102, 241, 0.7);
  }
  50% {
    background-color: rgba(79, 70, 229, 0.9);
  }
}

/* Remove the old animated-image class and replace with new styles */
.animated-preview {
  background: rgba(15, 23, 42, 0.8); /* slate-900/80 */
  padding: 2px;
  border-radius: 4px;
}

.animated-preview-modal {
  background: rgba(15, 23, 42, 0.8); /* slate-900/80 */
  padding: 4px;
  border-radius: 6px;
}

/* Remove the pulse-border animation since we're using a cleaner approach */
@keyframes pulse-border {
  0%, 100% {
    border-color: rgba(59, 130, 246, 0.8);
  }
  50% {
    border-color: rgba(96, 165, 250, 0.4);
  }
}

/* Update the animation indicator style */
.animation-indicator {
  background: linear-gradient(45deg, #4f46e5, #6366f1);
  box-shadow: 0 2px 4px rgba(79, 70, 229, 0.2);
}

/* Clean up unused animations */
@keyframes flash {
  0%, 100% {
    background-color: rgba(99, 102, 241, 0.7);
  }
  50% {
    background-color: rgba(79, 70, 229, 0.9);
  }
}

/* PDF container and canvas styles */
.pdf-container {
  max-height: 70vh;
  border: 1px solid rgba(30, 41, 59, 0.5);
}

.pdf-canvas-wrapper {
  min-height: 500px;
}

/* Make the modal content wider and taller for PDFs */
:deep(.modal-content-pdf-fullscreen) {
  max-width: 95vw;
  width: 95vw;
  height: 90vh;
  max-height: 90vh;
  display: flex;
  flex-direction: column;
}

:deep(.modal-content-pdf-fullscreen > div) {
  flex: 1;
  display: flex;
  flex-direction: column;
}

:deep(.modal-content-pdf-fullscreen .pdf-container) {
  min-height: calc(90vh - 140px);
  max-height: calc(90vh - 140px);
}

:deep(.pdf-modal-header) {
  padding: 0.75rem 1rem;
  background-color: rgba(15, 23, 42, 0.8);
  border-bottom-color: rgba(51, 65, 85, 0.5);
}
</style>