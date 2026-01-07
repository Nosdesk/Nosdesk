<script setup lang="ts">
import { ref, onMounted, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import PDFViewer from '@/components/ticketComponents/PDFViewer.vue';
import { useTitleManager } from '@/composables/useTitleManager';

const route = useRoute();
const router = useRouter();
const titleManager = useTitleManager();

// State
const pdfSrc = ref<string>('');
const filename = ref<string>('Document');
const startPage = ref<number>(1);
const isLoading = ref<boolean>(true);
const errorMessage = ref<string | null>(null);

// Parse query parameters
onMounted(() => {
  try {
    isLoading.value = true;

    // Get source from query parameters
    if (route.query.src) {
      pdfSrc.value = decodeURIComponent(route.query.src as string);
    } else {
      throw new Error('No PDF source provided');
    }

    // Get filename
    if (route.query.filename) {
      filename.value = decodeURIComponent(route.query.filename as string);
    }

    // Get starting page if provided
    if (route.query.page && !isNaN(Number(route.query.page))) {
      startPage.value = Number(route.query.page);
    }

    // Set document title (just filename, SiteHeader shows the icon)
    titleManager.setCustomTitle(filename.value);

    isLoading.value = false;
  } catch (error) {
    console.error('Error loading PDF:', error);
    errorMessage.value = error instanceof Error ? error.message : 'Failed to load PDF';
    isLoading.value = false;
  }
});

const handlePdfReady = () => {
  console.log('PDF loaded successfully');
};

const handlePdfError = (error: unknown) => {
  console.error('PDF loading error:', error);
  const err = error as { message?: string };
  errorMessage.value = 'Failed to load PDF: ' + (err?.message || 'Unknown error');
};

// Handle back navigation
const goBack = () => {
  if (window.history.length > 1) {
    router.back();
  } else {
    router.push('/');
  }
};

// Share document - copy link to clipboard
const shareDocument = async () => {
  try {
    await navigator.clipboard.writeText(window.location.href);
    // Could add a toast notification here
    console.log('Link copied to clipboard');
  } catch (err) {
    console.error('Failed to copy link:', err);
  }
};

// Update title when filename changes
watch(filename, (newFilename) => {
  titleManager.setCustomTitle(newFilename);
});
</script>

<template>
  <div class="bg-app flex flex-col h-full">
    <!-- Toolbar with back and share -->
    <div class="bg-surface border-b border-default px-4 py-2">
      <div class="flex items-center justify-between max-w-5xl mx-auto">
        <!-- Back button -->
        <button
          @click="goBack"
          class="flex items-center gap-2 px-3 py-1.5 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
        >
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 19l-7-7m0 0l7-7m-7 7h18" />
          </svg>
          <span class="text-sm font-medium">Back</span>
        </button>

        <!-- Share button -->
        <button
          @click="shareDocument"
          class="flex items-center gap-2 px-3 py-1.5 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
          title="Copy link to clipboard"
        >
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8.684 13.342C8.886 12.938 9 12.482 9 12c0-.482-.114-.938-.316-1.342m0 2.684a3 3 0 110-2.684m0 2.684l6.632 3.316m-6.632-6l6.632-3.316m0 0a3 3 0 105.367-2.684 3 3 0 00-5.367 2.684zm0 9.316a3 3 0 105.368 2.684 3 3 0 00-5.368-2.684z" />
          </svg>
          <span class="text-sm font-medium hidden sm:inline">Share</span>
        </button>
      </div>
    </div>

    <!-- Main content area -->
    <div class="flex-grow overflow-hidden">
      <!-- Loading state -->
      <div v-if="isLoading" class="h-full flex items-center justify-center">
        <div class="flex flex-col items-center gap-3">
          <svg class="animate-spin h-8 w-8 text-accent" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
          <span class="text-secondary">Loading PDF document...</span>
        </div>
      </div>

      <!-- Error state -->
      <div v-else-if="errorMessage" class="h-full flex items-center justify-center p-6">
        <div class="bg-status-error/30 text-status-error p-6 rounded-lg border border-status-error/70 shadow-lg max-w-md">
          <div class="flex items-start gap-4">
            <svg class="w-8 h-8 text-status-error flex-shrink-0" viewBox="0 0 20 20" fill="currentColor">
              <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z" clip-rule="evenodd" />
            </svg>
            <div>
              <h3 class="text-lg font-semibold text-status-error mb-2">Error Loading PDF</h3>
              <p>{{ errorMessage }}</p>
              <button
                @click="goBack"
                class="mt-4 bg-status-error/70 hover:bg-status-error/60 text-white px-4 py-2 rounded transition-colors"
              >
                Go Back
              </button>
            </div>
          </div>
        </div>
      </div>

      <!-- PDF Viewer -->
      <div v-else class="h-full overflow-hidden">
        <PDFViewer
          :src="pdfSrc"
          :filename="filename"
          :initialPage="startPage"
          class="h-full w-full"
          @ready="handlePdfReady"
          @error="handlePdfError"
        />
      </div>
    </div>
  </div>
</template>

<style scoped>
/* Ensure content area fills available space without overflow */
.flex-grow {
  min-height: 0;
}
</style> 