<script setup lang="ts">
import { computed, ref, onMounted } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { performBack } from '@/router/navigation';
import { useFluent } from 'fluent-vue';
import PDFViewer from '@/components/ticketComponents/PDFViewer.vue';
import BackButton from '@/components/common/BackButton.vue';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';
import Button from '@/components/common/Button.vue';
import { usePageTitle } from '@/composables/usePageTitle';
import { classifyPdfError, type PdfFailure } from '@/utils/pdfjs';

const route = useRoute();
const router = useRouter();
const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

// State
const pdfSrc = ref<string>('');
const filename = ref<string>(t('pdf-viewer-default-filename'));
const startPage = ref<number>(1);

// The tab is named after the PDF (SiteHeader shows the icon).
usePageTitle({ title: filename });
const isLoading = ref<boolean>(true);

// Why the PDF isn't showing. 'no-source' is a link without a file.
type ViewerFailure = PdfFailure | 'no-source';
const failure = ref<ViewerFailure | null>(null);
// Bumped to mount a fresh viewer, which loads the file again.
const viewerKey = ref(0);

const FAILURE_COPY: Record<ViewerFailure, { title: string; body: string; retry?: string }> = {
  'no-source': { title: 'pdf-viewer-error-title', body: 'pdf-viewer-error-no-source' },
  'not-found': { title: 'pdf-viewer-not-found-title', body: 'pdf-viewer-not-found-body' },
  invalid: { title: 'pdf-viewer-invalid-title', body: 'pdf-viewer-invalid-body' },
  network: { title: 'pdf-viewer-network-title', body: 'pdf-viewer-network-body', retry: 'pdf-viewer-try-again' },
  'password-cancelled': {
    title: 'pdf-viewer-locked-title',
    body: 'pdf-viewer-locked-body',
    retry: 'pdf-viewer-locked-enter-password',
  },
  unknown: { title: 'pdf-viewer-error-title', body: 'pdf-viewer-error-body', retry: 'pdf-viewer-try-again' },
};
const failureCopy = computed(() => (failure.value ? FAILURE_COPY[failure.value] : null));

// Parse query parameters
onMounted(() => {
  // The router has already decoded query values; decoding again throws on
  // a literal "%" (as in "50% off.pdf").
  if (typeof route.query.src === 'string' && route.query.src) {
    pdfSrc.value = route.query.src;
  } else {
    failure.value = 'no-source';
  }

  if (typeof route.query.filename === 'string' && route.query.filename) {
    filename.value = route.query.filename;
  }

  // Get starting page if provided
  if (route.query.page && !isNaN(Number(route.query.page))) {
    startPage.value = Number(route.query.page);
  }

  isLoading.value = false;
});

const handlePdfError = (error: unknown) => {
  failure.value = classifyPdfError(error);
  if (failure.value !== 'password-cancelled') {
    console.error('PDF loading error:', error);
  }
};

const retry = () => {
  failure.value = null;
  viewerKey.value++;
};

// Handle back navigation via the shared intelligent policy (in-app pop, else
// hierarchical parent) instead of the unreliable window.history.length check.
const goBack = () => performBack(router, route);

// Share document - copy link to clipboard
const shareDocument = async () => {
  try {
    await navigator.clipboard.writeText(window.location.href);
    // Could add a toast notification here
  } catch (err) {
    console.error('Failed to copy link:', err);
  }
};
</script>

<template>
  <div class="bg-app flex flex-col h-full">
    <!-- Toolbar with back and share -->
    <div class="bg-surface border-b border-default px-4 py-2">
      <div class="flex items-center justify-between max-w-5xl mx-auto">
        <!-- Back button -->
        <BackButton :label="$t('pdf-viewer-back')" />

        <!-- Share button -->
        <button
          type="button"
          @click="shareDocument"
          class="flex items-center gap-2 px-3 py-1.5 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
          :title="$t('pdf-viewer-share-tooltip')"
        >
          <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8.684 13.342C8.886 12.938 9 12.482 9 12c0-.482-.114-.938-.316-1.342m0 2.684a3 3 0 110-2.684m0 2.684l6.632 3.316m-6.632-6l6.632-3.316m0 0a3 3 0 105.367-2.684 3 3 0 00-5.367 2.684zm0 9.316a3 3 0 105.368 2.684 3 3 0 00-5.368-2.684z" />
          </svg>
          <span class="text-sm font-medium hidden sm:inline">{{ $t('pdf-viewer-share') }}</span>
        </button>
      </div>
    </div>

    <!-- Main content area -->
    <div class="flex-grow overflow-hidden">
      <!-- Loading state -->
      <div v-if="isLoading" class="h-full flex items-center justify-center">
        <div class="flex flex-col items-center gap-3">
          <Spinner size="lg" class="text-accent" />
          <span class="text-secondary">{{ $t('pdf-viewer-loading') }}</span>
        </div>
      </div>

      <!-- Error state -->
      <div v-else-if="failureCopy" class="h-full flex items-center justify-center p-6">
        <div
          role="alert"
          class="bg-surface border border-default p-6 rounded-lg shadow-lg max-w-md flex items-start gap-4"
          data-testid="pdf-viewer-failure"
        >
          <Icon
            :name="failure === 'password-cancelled' ? 'lock' : 'warning'"
            size="lg"
            :class="failure === 'password-cancelled' ? 'text-secondary flex-shrink-0' : 'text-status-error flex-shrink-0'"
          />
          <div class="flex flex-col gap-2">
            <h3 class="text-lg font-semibold text-primary">{{ $t(failureCopy.title) }}</h3>
            <p class="text-secondary">{{ $t(failureCopy.body) }}</p>
            <div class="flex flex-wrap gap-2 mt-2">
              <Button v-if="failureCopy.retry" @click="retry">
                {{ $t(failureCopy.retry) }}
              </Button>
              <Button variant="ghost" @click="goBack">
                {{ $t('pdf-viewer-error-go-back') }}
              </Button>
            </div>
          </div>
        </div>
      </div>

      <!-- PDF Viewer -->
      <div v-else class="h-full overflow-hidden">
        <PDFViewer
          :key="viewerKey"
          :src="pdfSrc"
          :filename="filename"
          :initialPage="startPage"
          class="h-full w-full"
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