<!-- The PDF overlay's contents: a header with the file's name, close and
share, and the viewer (or why it can't show the file). Mounted only while
the overlay is open, so the tab title is the PDF's for exactly that long. -->
<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { DialogClose, DialogTitle } from 'reka-ui'
import { useToastStore } from '@nosdesk/core/stores/toast'
import Icon from '@/components/common/Icon.vue'
import IconButton from '@/components/common/IconButton.vue'
import PDFViewer from '@/components/ticketComponents/PDFViewer.vue'
import PdfFailurePanel, { type PdfViewerFailure } from '@/components/ticketComponents/PdfFailurePanel.vue'
import type { PdfOverlaySource } from '@/composables/usePdfOverlay'
import { usePageTitle } from '@/composables/usePageTitle'
import { isTauriRuntime } from '@/platform'
import { convertToAuthenticatedPath } from '@/services/fileService'
import { classifyPdfError } from '@/utils/pdfjs'

const props = defineProps<{
  /** The file to show; null for a link that names no file the overlay may open. */
  source: PdfOverlaySource | null
}>()

const emit = defineEmits<{ close: [] }>()

const { $t } = useFluent()

const name = computed(() => props.source?.name ?? $t('pdf-viewer-default-filename'))
usePageTitle({ title: name })

// What the viewer fetches: a saved file through the authenticated file
// endpoint (the asset scheme in the app), an unsaved one by its blob URL.
const fetchSrc = computed(() => {
  const src = props.source?.src
  if (!src) return ''
  return src.startsWith('blob:') ? src : convertToAuthenticatedPath(src)
})

const failure = ref<PdfViewerFailure | null>(props.source ? null : 'no-source')
// Bumped to mount a fresh viewer, which loads the file again.
const viewerKey = ref(0)

watch(
  () => props.source?.src,
  (src) => {
    failure.value = src ? null : 'no-source'
    viewerKey.value++
  },
)

const handleError = (error: unknown) => {
  failure.value = classifyPdfError(error)
  if (failure.value !== 'password-cancelled') console.error('PDF loading error:', error)
}

const retry = () => {
  failure.value = null
  viewerKey.value++
}

// A saved file's link opens this view with the PDF on top. In the app the
// page address isn't one anyone else can open, so sharing waits for the
// native share sheet there.
const canShare = computed(() => !!props.source && !props.source.local && !isTauriRuntime())
const toast = useToastStore()
const copyLink = async () => {
  try {
    await navigator.clipboard.writeText(window.location.href)
    toast.success($t('pdf-viewer-link-copied'))
  } catch {
    toast.error($t('pdf-viewer-link-copy-failed'))
  }
}
</script>

<template>
  <div class="pdf-overlay-body flex flex-col h-full min-h-0">
    <header class="pdf-overlay-header flex items-center gap-2 px-2 sm:px-4 py-2 bg-surface border-b border-default">
      <DialogClose
        class="p-2 rounded-lg text-secondary hover:text-primary hover:bg-surface-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        :aria-label="$t('pdf-viewer-close')"
        data-testid="pdf-overlay-close"
      >
        <Icon name="close" />
      </DialogClose>
      <DialogTitle as="h2" class="flex-1 min-w-0 truncate text-sm font-medium text-primary">
        {{ name }}
      </DialogTitle>
      <IconButton
        v-if="canShare"
        icon="link"
        size="sm"
        :label="$t('pdf-viewer-copy-link')"
        data-testid="pdf-overlay-copy-link"
        @click="copyLink"
      />
    </header>

    <div class="flex-1 min-h-0 bg-app">
      <PdfFailurePanel v-if="failure" :failure="failure" @retry="retry" @close="emit('close')" />
      <PDFViewer
        v-else
        :key="viewerKey"
        :src="fetchSrc"
        :filename="name"
        :initial-page="source?.page ?? undefined"
        class="h-full w-full"
        @error="handleError"
      />
    </div>
  </div>
</template>

<style scoped>
/* Clear of the notch and the home indicator in the mobile app. */
.pdf-overlay-header {
  padding-top: max(0.5rem, env(safe-area-inset-top));
}

.pdf-overlay-body {
  padding-bottom: env(safe-area-inset-bottom);
}
</style>
