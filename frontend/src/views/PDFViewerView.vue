<script setup lang="ts">
import { ref, onMounted, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import PDFViewer from '@/components/ticketComponents/PDFViewer.vue';
import { useTitleManager } from '@/composables/useTitleManager';
import BackButton from '@/components/common/BackButton.vue';

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
    
    // Set document title
    titleManager.setCustomTitle(`PDF - ${filename.value}`);
    
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
    // If no history, navigate to a safe default
    router.push('/');
  }
};

// Update title when filename changes
watch(filename, (newFilename) => {
  titleManager.setCustomTitle(`PDF - ${newFilename}`);
});
</script>

<template>
  <div class="bg-app flex flex-col h-full">
    <!-- Header with back button -->
    <div class="bg-gradient-to-r from-app to-surface border-b border-default px-6 py-3">
      <div class="flex items-center justify-between">
        <!-- Back button -->
        <BackButton @click="goBack" label="Back" class="hover:scale-105 transition-transform duration-200" />
        
        <!-- Document title -->
        <h1 class="text-primary font-medium text-lg flex items-center">
          <svg class="w-5 h-5 text-status-error mr-2" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M4 4a2 2 0 012-2h4.586A2 2 0 0112 2.586L15.414 6A2 2 0 0116 7.414V16a2 2 0 01-2 2H6a2 2 0 01-2-2V4zm2 6a1 1 0 011-1h6a1 1 0 110 2H7a1 1 0 01-1-1zm1 3a1 1 0 100 2h6a1 1 0 100-2H7z" clip-rule="evenodd" />
          </svg>
          {{ filename }}
        </h1>
        
        <!-- Spacer to balance the layout -->
        <div class="w-24"></div>
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
      <div v-else class="h-full pb-4 px-4">
        <PDFViewer 
          :src="pdfSrc" 
          :filename="filename"
          :initialPage="startPage"
          class="h-full max-w-6xl mx-auto"
          @ready="handlePdfReady"
          @error="handlePdfError"
        />
      </div>
    </div>
  </div>
</template>

<style scoped>
/* Remove margin/padding from root container to maximize space */
:deep(.pdf-container) {
  min-height: calc(100vh - 180px);
  max-height: calc(100vh - 180px);
}

/* Full viewport height minus header */
.overflow-hidden {
  height: calc(100vh - 56px);
}
</style> 