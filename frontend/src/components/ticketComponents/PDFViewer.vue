<!-- PDFViewer.vue -->
<script setup lang="ts">
import { ref, shallowRef, onMounted, onBeforeUnmount, nextTick, watchEffect, computed } from 'vue';
import { useRouter, useRoute } from 'vue-router';

interface Props {
  src: string;
  filename: string;
  initialPage?: number;
}

const props = withDefaults(defineProps<Props>(), {
  initialPage: 1
});

const emit = defineEmits<{
  (e: 'ready'): void;
  (e: 'error', error: any): void;
}>();

const router = useRouter();
const route = useRoute();

// PDF viewing state
const pdfDocument = shallowRef<any>(null);
const pdfPage = ref<number>(props.initialPage);
const pdfTotalPages = ref<number>(0);
const pdfScale = ref<number>(1.0);
const initialFitScale = ref<number>(1.0); // Store initial fit scale
const mainCanvas = ref<HTMLCanvasElement | null>(null);
const transitionCanvas = ref<HTMLCanvasElement | null>(null);
const pdfContainer = ref<HTMLElement | null>(null);
const isLoading = ref<boolean>(false);
const pageInput = ref<string>(props.initialPage.toString());
const hasWindowFocus = ref<boolean>(true);
const currentRenderTask = shallowRef<any>(null);
const secondaryRenderTask = shallowRef<any>(null);
const pageRendering = ref<boolean>(false); // Flag to indicate if a page is being rendered
const pageNumPending = ref<number | null>(null); // Cache the pending page number
const isTransitioning = ref<boolean>(false);
const lastRenderedPage = ref<number | null>(null);

// Buffer system - preload pages into offscreen canvases
const pageBuffer = ref<Map<number, HTMLCanvasElement>>(new Map());
const MAX_BUFFER_SIZE = 10; // Store more pages in buffer
const bufferInProgress = ref<Set<number>>(new Set());

// Determine if in fullscreen view
const isFullscreenView = computed(() => {
  return route.path === '/pdf-viewer';
});

// Initialize the PDF.js library
const initPdfJs = async () => {
  const pdfjsLib = await import('pdfjs-dist');
  // Configure worker
  pdfjsLib.GlobalWorkerOptions.workerSrc = new URL('pdfjs-dist/build/pdf.worker.mjs', import.meta.url).href;
  return pdfjsLib;
};

// Function to fade in transition canvas
const fadeInTransitionCanvas = () => {
  if (!transitionCanvas.value) return;
  
  isTransitioning.value = true;
  
  // Set transition canvas to visible with fade-in
  transitionCanvas.value.style.opacity = '1';
  
  // After transition completes
  setTimeout(() => {
    if (mainCanvas.value && transitionCanvas.value) {
      // Copy transition canvas to main canvas
      const mainCtx = mainCanvas.value.getContext('2d');
      if (mainCtx) {
        mainCanvas.value.width = transitionCanvas.value.width;
        mainCanvas.value.height = transitionCanvas.value.height;
        mainCanvas.value.style.width = transitionCanvas.value.style.width;
        mainCanvas.value.style.height = transitionCanvas.value.style.height;
        
        mainCtx.clearRect(0, 0, mainCanvas.value.width, mainCanvas.value.height);
        mainCtx.drawImage(transitionCanvas.value, 0, 0);
      }
      
      // Hide transition canvas for next use
      transitionCanvas.value.style.opacity = '0';
    }
    
    isTransitioning.value = false;
  }, 250); // Match with CSS transition duration
};

// Helper function to create and render to an offscreen canvas
const renderToOffscreenCanvas = async (pageNum: number) => {
  if (!pdfDocument.value || bufferInProgress.value.has(pageNum) || pageBuffer.value.has(pageNum)) {
    return; // Skip if already buffered or in progress or no document
  }
  
  // Add to in-progress set
  bufferInProgress.value.add(pageNum);
  
  try {
    const pdfjsLib = await initPdfJs();
    
    // Get page
    const page = await pdfDocument.value.getPage(pageNum);
    
    // Create offscreen canvas
    const offscreenCanvas = document.createElement('canvas');
    const context = offscreenCanvas.getContext('2d');
    if (!context) {
      throw new Error('Could not get canvas context');
    }
    
    // Get container dimensions for scaling
    const containerWidth = pdfContainer.value?.clientWidth || window.innerWidth * 0.8;
    const containerHeight = pdfContainer.value?.clientHeight || window.innerHeight * 0.7;
    
    // Apply user zoom on top of fit scale
    const scaledViewport = page.getViewport({ scale: initialFitScale.value * pdfScale.value });
    
    // Set canvas dimensions with device pixel ratio
    const pixelRatio = window.devicePixelRatio || 1;
    offscreenCanvas.height = scaledViewport.height * pixelRatio;
    offscreenCanvas.width = scaledViewport.width * pixelRatio;
    
    // Scale the context to match the device pixel ratio
    context.scale(pixelRatio, pixelRatio);
    
    // Render the page
    const renderTask = page.render({
      canvasContext: context,
      viewport: scaledViewport
    });
    
    await renderTask.promise;
    
    // Store in buffer
    pageBuffer.value.set(pageNum, offscreenCanvas);
    
    // Clean buffer if it gets too large
    if (pageBuffer.value.size > MAX_BUFFER_SIZE) {
      // Keep important pages (current, adjacent, and a few commonly accessed)
      const currentPage = pdfPage.value;
      const keysToKeep = [
        currentPage, 
        Math.max(1, currentPage - 1), 
        Math.min(pdfTotalPages.value, currentPage + 1),
        Math.max(1, currentPage - 2),
        Math.min(pdfTotalPages.value, currentPage + 2),
        1, // first page
        pdfTotalPages.value // last page
      ].filter((value, index, self) => self.indexOf(value) === index); // Remove duplicates
      
      // Remove pages not in keysToKeep
      for (const bufferedPage of pageBuffer.value.keys()) {
        if (!keysToKeep.includes(bufferedPage)) {
          pageBuffer.value.delete(bufferedPage);
        }
      }
    }
  } catch (error) {
    // Don't log cancellation errors for preloading
    if (error && typeof error === 'object' && 'name' in error && error.name !== 'RenderingCancelledException') {
      console.error(`Error buffering page ${pageNum}:`, error);
    }
  } finally {
    bufferInProgress.value.delete(pageNum);
  }
};

// Preload adjacent pages
const preloadAdjacentPages = () => {
  const currentPage = pdfPage.value;
  
  // Preload next 2 pages
  for (let i = 1; i <= 2; i++) {
    const nextPage = currentPage + i;
    if (nextPage <= pdfTotalPages.value) {
      renderToOffscreenCanvas(nextPage);
    }
  }
  
  // Preload previous 2 pages
  for (let i = 1; i <= 2; i++) {
    const prevPage = currentPage - i;
    if (prevPage >= 1) {
      renderToOffscreenCanvas(prevPage);
    }
  }
};

// Function to render a page to the transition canvas
const renderToTransitionCanvas = async (pageNum: number): Promise<boolean> => {
  if (!pdfDocument.value || !transitionCanvas.value) return false;
  
  // Safety check - if the requested page is the last rendered page and
  // not in the middle of a transition, no need to re-render
  if (pageNum === lastRenderedPage.value && !isTransitioning.value) {
    return true;
  }
  
  try {
    const pdfjsLib = await initPdfJs();
    
    // Get page with error handling
    let page;
    try {
      page = await pdfDocument.value.getPage(pageNum);
    } catch (error) {
      console.error('Error getting page, trying to reload document:', error);
      // If there's an error, try re-fetching the document
      const response = await fetch(props.src);
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`);
      }
      const arrayBuffer = await response.arrayBuffer();
      const loadingTask = pdfjsLib.getDocument({ data: new Uint8Array(arrayBuffer) });
      pdfDocument.value = await loadingTask.promise;
      pdfTotalPages.value = pdfDocument.value.numPages;
      page = await pdfDocument.value.getPage(pageNum);
    }
    
    // Calculate scale for the first page or when initialFitScale is not set
    if ((pageNum === 1 || initialFitScale.value === 1.0) && pdfContainer.value) {
      const viewport = page.getViewport({ scale: 1.0 });
      const containerWidth = pdfContainer.value.clientWidth || 800;
      const containerHeight = pdfContainer.value.clientHeight || window.innerHeight * 0.7;
      
      const scaleByWidth = containerWidth / viewport.width;
      const scaleByHeight = containerHeight / viewport.height;
      
      // Choose the smaller scale to fit both dimensions
      initialFitScale.value = Math.min(scaleByWidth, scaleByHeight) * 0.95; // Add a small margin
    }
    
    const context = transitionCanvas.value.getContext('2d');
    if (!context) throw new Error('Could not get canvas context');
    
    // Get scaled viewport
    const scaledViewport = page.getViewport({ scale: initialFitScale.value * pdfScale.value });
    
    // Set canvas dimensions with device pixel ratio
    const pixelRatio = window.devicePixelRatio || 1;
    transitionCanvas.value.width = scaledViewport.width * pixelRatio;
    transitionCanvas.value.height = scaledViewport.height * pixelRatio;
    
    // Set canvas style dimensions
    transitionCanvas.value.style.width = `${scaledViewport.width}px`;
    transitionCanvas.value.style.height = `${scaledViewport.height}px`;
    
    // Clear canvas
    context.clearRect(0, 0, transitionCanvas.value.width, transitionCanvas.value.height);
    
    // Scale context for high DPI
    context.scale(pixelRatio, pixelRatio);
    
    // Render the page
    const renderTask = page.render({
      canvasContext: context,
      viewport: scaledViewport
    });
    
    secondaryRenderTask.value = renderTask;
    
    await renderTask.promise;
    secondaryRenderTask.value = null;
    
    // Add to buffer for future use
    const offscreenCanvas = document.createElement('canvas');
    offscreenCanvas.width = transitionCanvas.value.width;
    offscreenCanvas.height = transitionCanvas.value.height;
    const offContext = offscreenCanvas.getContext('2d');
    if (offContext) {
      offContext.drawImage(transitionCanvas.value, 0, 0);
      pageBuffer.value.set(pageNum, offscreenCanvas);
    }
    
    // Update last rendered page
    lastRenderedPage.value = pageNum;
    
    return true;
  } catch (error) {
    if (error && typeof error === 'object' && 'name' in error && error.name === 'RenderingCancelledException') {
      console.log('Transition rendering cancelled:', 'message' in error ? error.message : 'Unknown reason');
    } else {
      console.error('Error rendering to transition canvas:', error);
    }
    return false;
  }
};

// Function to load and render PDF 
const loadPdfPreview = async (src: string, pageNumber: number = 1, isNewDocument = false) => {
  // Store the target page number for reliable navigation
  const targetPageNumber = pageNumber;
  
  // Validate page number
  if (pdfDocument.value && (targetPageNumber < 1 || targetPageNumber > pdfTotalPages.value)) {
    console.warn(`Invalid page number: ${targetPageNumber}`);
    return;
  }
  
  // If a page is already rendering, queue this page number and return
  if (pageRendering.value) {
    pageNumPending.value = targetPageNumber;
    return;
  }
  
  pageRendering.value = true; // Set rendering flag
  
  // Don't show loading indicator for rapid navigation if possible
  if (isNewDocument || (!pageBuffer.value.has(targetPageNumber) && !isTransitioning.value)) {
    isLoading.value = true;
  }
  
  try {
    // Wait for canvas references to be ready
    await nextTick();
    
    // Validate canvas elements
    if (!mainCanvas.value || !transitionCanvas.value) {
      throw new Error('Canvas elements not found');
    }
    
    // Cancel any existing render task
    if (currentRenderTask.value) {
      try {
        await currentRenderTask.value.cancel();
        currentRenderTask.value = null;
      } catch (err) {
        console.log('Error cancelling primary render task:', err);
      }
    }
    
    if (secondaryRenderTask.value) {
      try {
        await secondaryRenderTask.value.cancel();
        secondaryRenderTask.value = null;
      } catch (err) {
        console.log('Error cancelling secondary render task:', err);
      }
    }
    
    // Load PDF document if not already loaded
    if (!pdfDocument.value || isNewDocument) {
      try {
        // Clear buffer if loading new document
        if (isNewDocument) {
          pageBuffer.value.clear();
          bufferInProgress.value.clear();
          lastRenderedPage.value = null;
        }
        
        const pdfjsLib = await initPdfJs();
        
        // Fetch the PDF with authentication and convert to array buffer
        console.log('Fetching PDF from:', src);
        const response = await fetch(src);
        
        if (!response.ok) {
          throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        
        const arrayBuffer = await response.arrayBuffer();
        console.log('PDF fetched successfully, size:', arrayBuffer.byteLength, 'bytes');
        
        // Load PDF from the array buffer instead of URL
        const loadingTask = pdfjsLib.getDocument({ data: new Uint8Array(arrayBuffer) });
        pdfDocument.value = await loadingTask.promise;
        pdfTotalPages.value = pdfDocument.value.numPages;
        console.log('PDF loaded successfully, pages:', pdfTotalPages.value);
      } catch (error) {
        console.error('Error loading PDF document:', error);
        emit('error', error);
        throw error;
      }
    }
    
    let renderSuccess = false;
    
    // Check if page is in buffer
    if (pageBuffer.value.has(targetPageNumber) && !isNewDocument && !isTransitioning.value) {
      const bufferedCanvas = pageBuffer.value.get(targetPageNumber)!;
      
      // Copy buffered canvas to transition canvas
      const transCtx = transitionCanvas.value.getContext('2d');
      if (transCtx) {
        // Set dimensions
        transitionCanvas.value.width = bufferedCanvas.width;
        transitionCanvas.value.height = bufferedCanvas.height;
        transitionCanvas.value.style.width = `${bufferedCanvas.width / (window.devicePixelRatio || 1)}px`;
        transitionCanvas.value.style.height = `${bufferedCanvas.height / (window.devicePixelRatio || 1)}px`;
        
        // Draw buffered image
        transCtx.clearRect(0, 0, transitionCanvas.value.width, transitionCanvas.value.height);
        transCtx.drawImage(bufferedCanvas, 0, 0);
        
        // Update page state
        pdfPage.value = targetPageNumber;
        pageInput.value = targetPageNumber.toString();
        lastRenderedPage.value = targetPageNumber;
        
        // Transition with fade effect
        fadeInTransitionCanvas();
        renderSuccess = true;
      }
    } else {
      // Render directly to transition canvas
      renderSuccess = await renderToTransitionCanvas(targetPageNumber);
      
      if (renderSuccess) {
        // Update page state
        pdfPage.value = targetPageNumber;
        pageInput.value = targetPageNumber.toString();
        
        // Transition with fade effect
        fadeInTransitionCanvas();
      }
    }
    
    if (renderSuccess) {
      // Signal completion
      emit('ready');
      
      // Preload adjacent pages for smoother future navigation
      setTimeout(() => preloadAdjacentPages(), 100);
    } else {
      // If rendering failed but wasn't cancelled, retry once
      console.log('Retrying render for page', targetPageNumber);
      setTimeout(() => {
        if (pdfPage.value !== targetPageNumber) {
          loadPdfPreview(src, targetPageNumber);
        }
      }, 200);
    }
    
  } catch (error) {
    if (error && typeof error === 'object' && 'name' in error && error.name === 'RenderingCancelledException') {
      console.log('Primary rendering cancelled:', 'message' in error ? error.message : 'Unknown reason');
      
      // If rendering was cancelled but we were trying to go to a specific page,
      // retry after a short delay
      setTimeout(() => {
        if (pdfPage.value !== targetPageNumber) {
          loadPdfPreview(props.src, targetPageNumber);
        }
      }, 300);
    } else {
      console.error('Error in loadPdfPreview:', error);
      emit('error', error);
    }
  } finally {
    isLoading.value = false;
    pageRendering.value = false; // Clear rendering flag
    
    // Process any pending page requests
    if (pageNumPending.value !== null) {
      const pendingPage = pageNumPending.value;
      pageNumPending.value = null; // Reset pending page
      setTimeout(() => {
        loadPdfPreview(props.src, pendingPage);
      }, 50); // Short delay to prevent rapid rendering attempts
    }
  }
};

// Function to go to a specific page
const goToPage = () => {
  if (!pdfDocument.value) return;
  
  const pageNumber = parseInt(pageInput.value);
  if (!isNaN(pageNumber) && pageNumber >= 1 && pageNumber <= pdfTotalPages.value) {
    // Check if trying to go to current page
    if (pageNumber === pdfPage.value) {
      return;
    }
    loadPdfPreview(props.src, pageNumber);
  } else {
    // Reset to current page if invalid input
    pageInput.value = pdfPage.value.toString();
  }
};

const handlePageInputKeyDown = (event: KeyboardEvent) => {
  if (event.key === 'Enter') {
    goToPage();
    // Remove focus from input after entering page
    (event.target as HTMLInputElement).blur();
  }
};

const navigatePdf = (direction: 'next' | 'prev') => {
  if (!pdfDocument.value) return;
  
  let newPage: number;
  
  if (direction === 'next' && pdfPage.value < pdfTotalPages.value) {
    newPage = pdfPage.value + 1;
  } else if (direction === 'prev' && pdfPage.value > 1) {
    newPage = pdfPage.value - 1;
  } else {
    return; // Can't navigate further
  }
  
  // Update page input immediately for responsiveness
  pageInput.value = newPage.toString();
  
  // Load the new page
  loadPdfPreview(props.src, newPage);
};

const changeScale = (change: number) => {
  // Limit scale between 0.5 and 3.0
  const newScale = Math.max(0.5, Math.min(3.0, pdfScale.value + change));
  if (newScale !== pdfScale.value) {
    pdfScale.value = newScale;
    // Clear the buffer when scale changes
    pageBuffer.value.clear();
    loadPdfPreview(props.src, pdfPage.value);
  }
};

// Reset view function
const resetView = () => {
  pdfScale.value = 1.0;
  // Clear the buffer when scale changes
  pageBuffer.value.clear();
  loadPdfPreview(props.src, pdfPage.value);
};

// Open fullscreen PDF view
const openFullscreen = () => {
  const queryParams = {
    src: encodeURIComponent(props.src),
    filename: encodeURIComponent(props.filename),
    page: pdfPage.value
  };
  const queryString = Object.entries(queryParams)
    .map(([key, value]) => `${key}=${value}`)
    .join('&');
  
  router.push(`/pdf-viewer?${queryString}`);
};

// Handle keyboard navigation
const handleKeyDown = (event: KeyboardEvent) => {
  // Only handle key events when the viewer has focus and no input element is focused
  if (!hasWindowFocus.value || event.target instanceof HTMLInputElement) {
    return;
  }
  
  switch(event.key) {
    case 'ArrowLeft':
    case 'PageUp':
      navigatePdf('prev');
      event.preventDefault();
      break;
    case 'ArrowRight':
    case 'PageDown':
    case ' ': // Space
      navigatePdf('next');
      event.preventDefault();
      break;
    case 'Home':
      if (pdfDocument.value) {
        pageInput.value = '1';
        goToPage();
        event.preventDefault();
      }
      break;
    case 'End':
      if (pdfDocument.value && pdfTotalPages.value > 0) {
        pageInput.value = pdfTotalPages.value.toString();
        goToPage();
        event.preventDefault();
      }
      break;
    case '+':
    case '=':
      changeScale(0.1);
      event.preventDefault();
      break;
    case '-':
      changeScale(-0.1);
      event.preventDefault();
      break;
    case 'f':
      if (!isFullscreenView.value) {
        openFullscreen();
      } else {
        resetView();
      }
      event.preventDefault();
      break;
    case 'r':
      resetView();
      event.preventDefault();
      break;
  }
};

// Track window focus to only respond to keyboard shortcuts when the window is active
const handleWindowFocus = () => {
  hasWindowFocus.value = true;
};

const handleWindowBlur = () => {
  hasWindowFocus.value = false;
};

// Set up key listeners on mount
onMounted(() => {
  window.addEventListener('keydown', handleKeyDown);
  window.addEventListener('focus', handleWindowFocus);
  window.addEventListener('blur', handleWindowBlur);
  
  // Handle window resize and re-render the PDF
  const handleResize = () => {
    // Use a small timeout to avoid too many renders during resize
    if (pdfDocument.value) {
      // Clear buffer on resize
      pageBuffer.value.clear();
      loadPdfPreview(props.src, pdfPage.value);
    }
  };
  
  const debouncedResize = debounce(handleResize, 250);
  window.addEventListener('resize', debouncedResize);
  
  // Clean up resize listener on unmount
  onBeforeUnmount(() => {
    window.removeEventListener('resize', debouncedResize);
  });
});

// Simple debounce function
function debounce(fn: Function, delay: number) {
  let timeoutId: number | null = null;
  return function(...args: any[]) {
    if (timeoutId !== null) {
      clearTimeout(timeoutId);
    }
    timeoutId = window.setTimeout(() => {
      fn(...args);
      timeoutId = null;
    }, delay);
  };
}

// Clean up PDF resources and event listeners on component unmount
onBeforeUnmount(() => {
  // Cancel any pending render task
  if (currentRenderTask.value) {
    try {
      currentRenderTask.value.cancel();
    } catch (err) {
      console.log('Error cancelling render task during unmount:', err);
    }
  }
  
  // Clear buffers
  pageBuffer.value.clear();
  bufferInProgress.value.clear();
  
  // Remove event listeners
  window.removeEventListener('keydown', handleKeyDown);
  window.removeEventListener('focus', handleWindowFocus);
  window.removeEventListener('blur', handleWindowBlur);
  
  // Clean up PDF document
  if (pdfDocument.value) {
    try {
      // Use optional chaining to safely call destroy
      pdfDocument.value?.destroy?.();
    } catch (error) {
      console.error('Error destroying PDF document:', error);
    }
    pdfDocument.value = null;
  }
});

// Watch for src changes and reload PDF - using a cautious approach
watchEffect(() => {
  const currentSrc = props.src;
  if (currentSrc) {
    // Reset state when src changes - do this carefully to avoid reactivity issues
    pdfPage.value = props.initialPage;
    pageInput.value = props.initialPage.toString();
    pdfScale.value = 1.0; // Reset zoom
    
    // Don't modify pdfDocument directly in the watchEffect,
    // as it can cause reactivity issues with PDF.js
    nextTick(() => {
      // Set document to null in the next tick, outside the watcher
      pdfDocument.value = null;
      
      // Cancel any existing render task
      if (currentRenderTask.value) {
        try {
          currentRenderTask.value.cancel();
          currentRenderTask.value = null;
        } catch (err) {
          console.log('Error cancelling render task during src change:', err);
        }
      }
      
      // Clear buffer when src changes
      pageBuffer.value.clear();
      bufferInProgress.value.clear();
      
      // Then load the new document - passing true to indicate it's a new document
      loadPdfPreview(currentSrc, props.initialPage, true);
    });
  }
});
</script>

<template>
  <div class="pdf-viewer relative w-full focus:outline-none" tabindex="0">
    <!-- PDF loading indicator -->
    <div
      v-if="isLoading"
      class="absolute inset-0 bg-surface/70 flex items-center justify-center z-20"
    >
      <div class="flex flex-col items-center gap-3">
        <svg class="animate-spin h-8 w-8 text-accent" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
          <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
          <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
        </svg>
        <span class="text-sm text-secondary">Loading PDF...</span>
      </div>
    </div>

    <!-- PDF Canvas - contained in a better sized div with overflow handling -->
    <div
      ref="pdfContainer"
      class="pdf-container w-full h-full max-h-[calc(100vh-180px)] mx-auto overflow-auto bg-surface p-1 rounded"
    >
      <div class="pdf-canvas-wrapper">
        <!-- Multiple canvas layers for smooth transitions -->
        <div class="canvas-container">
          <canvas 
            ref="mainCanvas" 
            class="canvas-layer shadow-lg"
          ></canvas>
          <canvas 
            ref="transitionCanvas" 
            class="canvas-layer-transition shadow-lg"
          ></canvas>
        </div>
      </div>
    </div>
    
    <!-- PDF Controls -->
    <div class="mt-4 flex flex-wrap items-center justify-between w-full bg-surface-alt rounded p-2 gap-2">
      <!-- Page Navigation -->
      <div class="flex items-center gap-2">
        <button
          @click="navigatePdf('prev')"
          :disabled="pdfPage <= 1 || pageRendering"
          class="p-1.5 bg-surface-hover rounded text-secondary hover:bg-surface-hover disabled:opacity-50 disabled:cursor-not-allowed"
          title="Previous Page (Left Arrow)"
        >
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7" />
          </svg>
        </button>

        <!-- Page input field -->
        <div class="flex items-center gap-1">
          <input
            v-model="pageInput"
            type="text"
            inputmode="numeric"
            pattern="[0-9]*"
            class="w-12 bg-surface-hover text-center text-primary text-sm rounded p-1 focus:outline-none focus:ring-1 focus:ring-accent"
            @keydown="handlePageInputKeyDown"
            @blur="goToPage"
            title="Enter page number"
            :disabled="pageRendering"
          />
          <span class="text-sm text-secondary whitespace-nowrap">/ {{ pdfTotalPages }}</span>
        </div>

        <button
          @click="navigatePdf('next')"
          :disabled="pdfPage >= pdfTotalPages || pageRendering"
          class="p-1.5 bg-surface-hover rounded text-secondary hover:bg-surface-hover disabled:opacity-50 disabled:cursor-not-allowed"
          title="Next Page (Right Arrow)"
        >
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
          </svg>
        </button>
      </div>

      <!-- Zoom Controls -->
      <div class="flex items-center gap-2">
        <button
          @click="changeScale(-0.1)"
          class="p-1.5 bg-surface-hover rounded text-secondary hover:bg-surface-hover"
          title="Zoom Out (-)"
        >
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M20 12H4" />
          </svg>
        </button>
        <span class="text-sm text-secondary">{{ Math.round(pdfScale * 100) }}%</span>
        <button
          @click="changeScale(0.1)"
          class="p-1.5 bg-surface-hover rounded text-secondary hover:bg-surface-hover"
          title="Zoom In (+)"
        >
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
          </svg>
        </button>

        <!-- Toggle button based on view mode -->
        <button
          v-if="isFullscreenView"
          @click="resetView"
          class="p-1.5 bg-surface-hover rounded text-secondary hover:bg-surface-hover ml-1"
          title="Reset View (R)"
        >
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 12h18M12 3v18" />
          </svg>
        </button>
        <button
          v-else
          @click="openFullscreen"
          class="p-1.5 bg-surface-hover rounded text-secondary hover:bg-surface-hover ml-1"
          title="Fullscreen View (F)"
        >
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5v-4m0 4h-4m4 0l-5-5" />
          </svg>
        </button>
      </div>
    </div>

    <!-- Keyboard shortcuts help -->
    <div class="text-xs text-tertiary mt-2 text-center">
      <p v-if="isFullscreenView">Keyboard: ←/→ arrows to navigate • +/- to zoom • R to reset view</p>
      <p v-else>Keyboard: ←/→ arrows to navigate • +/- to zoom • F for fullscreen</p>
    </div>
    
    <!-- Download Button -->
    <div class="mt-4 flex justify-center">
      <a
        :href="src"
        target="_blank"
        :download="filename"
        class="px-4 py-2 bg-accent text-white text-sm rounded hover:opacity-90 transition-colors flex items-center gap-2"
      >
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4"></path>
        </svg>
        Download PDF
      </a>
    </div>
  </div>
</template>

<style scoped>
.pdf-container {
  min-height: 60vh;
  border: 1px solid rgba(30, 41, 59, 0.5);
}

.pdf-canvas-wrapper {
  min-height: 400px;
  /* Center canvas in the available space */
  display: flex;
  justify-content: center;
  align-items: center;
}

.canvas-container {
  position: relative;
  display: flex;
  justify-content: center;
  align-items: center;
}

/* Canvas layers for transitions */
.canvas-layer {
  display: block;
  margin: 0 auto;
  width: 100%;
  max-width: 100%;
  height: auto !important;
  object-fit: contain;
}

.canvas-layer-transition {
  position: absolute;
  top: 0;
  left: 0;
  z-index: 10;
  opacity: 0;
  transition: opacity 200ms ease-in-out;
  display: block;
  margin: 0 auto;
  width: 100%;
  max-width: 100%;
  height: auto !important;
  object-fit: contain;
}

/* Animation for the loading spinner */
@keyframes spin {
  0% {
    transform: rotate(0deg);
  }
  100% {
    transform: rotate(360deg);
  }
}

.animate-spin {
  animation: spin 1s linear infinite;
}

/* Hide default number input spinner buttons */
input::-webkit-outer-spin-button,
input::-webkit-inner-spin-button {
  -webkit-appearance: none;
  margin: 0;
}

/* Firefox */
input[type=number] {
  -moz-appearance: textfield;
}

/* Add focus indicator for keyboard navigation */
.pdf-viewer:focus {
  outline: 2px solid rgba(59, 130, 246, 0.5);
  outline-offset: 2px;
}
</style> 