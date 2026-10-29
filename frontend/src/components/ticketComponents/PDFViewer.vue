<!-- PDFViewer.vue - pdf.js's own viewer inside our toolbar.

pdf.js's PDFViewer owns everything inside the scroll container: the render
queue, text layer (selection, browser find, screen readers), links, pinch
zoom and canvas limits. Vue owns the toolbar around it and talks to it
through its EventBus. -->
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import type { PDFDocumentProxy } from 'pdfjs-dist'
import type { EventBus, PDFFindController, PDFLinkService, PDFViewer } from 'pdfjs-dist/web/pdf_viewer.mjs'
import { useToastStore } from '@nosdesk/core/stores/toast'
import BaseDropdown, { type DropdownOption } from '@/components/common/BaseDropdown.vue'
import IconButton from '@/components/common/IconButton.vue'
import Icon from '@/components/common/Icon.vue'
import Spinner from '@/components/common/Spinner.vue'
import PdfFindBar, { type PdfFindRequest } from '@/components/ticketComponents/PdfFindBar.vue'
import PdfPasswordPrompt from '@/components/ticketComponents/PdfPasswordPrompt.vue'
import { usePdfDocument } from '@/composables/usePdfDocument'
import { isTauriRuntime, openExternalUrl, shareFile } from '@/platform'
import { loadPdfjs, loadPdfjsViewer, pdfjsImagesUrl, type PdfPasswordReason } from '@/utils/pdfjs'
import { createPdfjsL10n } from '@/utils/pdfjsL10n'
import { printPdfDocument } from '@/utils/printPdf'

interface Props {
  src: string
  filename: string
  /** The page to open at. */
  initialPage?: number
}

const props = defineProps<Props>()

const emit = defineEmits<{
  (e: 'ready'): void
  (e: 'error', error: unknown): void
  /** Escape was pressed in the viewer. */
  (e: 'close'): void
}>()

const { $t } = useFluent()

// ── Password prompt ─────────────────────────────────────────────
// The open prompt, if any. Its answer resolves the request pdf.js is waiting
// on; closing it (or leaving the viewer) answers null. After a submit it
// stays open, busy, until the document opens or pdf.js asks again.
interface PasswordPrompt {
  reason: PdfPasswordReason
  busy: boolean
  answer: (password: string | null) => void
}
const passwordPrompt = ref<PasswordPrompt | null>(null)

const requestPassword = (reason: PdfPasswordReason) =>
  new Promise<string | null>((resolve) => {
    passwordPrompt.value = { reason, busy: false, answer: resolve }
  })

const answerPassword = (password: string | null) => {
  const prompt = passwordPrompt.value
  if (!prompt) return
  passwordPrompt.value = password === null ? null : { ...prompt, busy: true, answer: () => {} }
  prompt.answer(password)
}

// ── Document ────────────────────────────────────────────────────
const pdf = usePdfDocument({ onPassword: requestPassword })

watch(pdf.isLoading, (loading) => {
  if (!loading && passwordPrompt.value?.busy) passwordPrompt.value = null
})
watch(pdf.error, (error) => {
  if (error) emit('error', error)
})
watch(
  () => props.src,
  (src) => {
    if (src) void pdf.load(src)
  },
  { immediate: true },
)

// ── pdf.js viewer ───────────────────────────────────────────────
const container = ref<HTMLDivElement | null>(null)
const viewerElement = ref<HTMLDivElement | null>(null)

let viewer: PDFViewer | null = null
let linkService: PDFLinkService | null = null
let findController: PDFFindController | null = null
// pdf.js's page and annotation labels, from our catalogue.
const l10n = createPdfjsL10n((key, args) => $t(key, args), document.documentElement.lang || navigator.language)
let eventBus: EventBus | null = null
let unmounted = false

const pageNumber = ref(1)
const pagesCount = ref(0)
const scale = ref(1)
/** The preset in use ('auto', 'page-width', 'page-fit'), or null for a number. */
const scalePreset = ref<string | null>(null)

// Hands the viewer the current document. Runs synchronously when the
// document changes, so the viewer lets go of the old one before
// usePdfDocument releases it.
const attachDocument = (doc: PDFDocumentProxy | null) => {
  if (!viewer || !linkService) return
  viewer.setDocument(doc as PDFDocumentProxy)
  linkService.setDocument(doc, null)
  findController?.setDocument(doc as PDFDocumentProxy)
  pagesCount.value = doc?.numPages ?? 0
}
watch(pdf.pdfDocument, attachDocument, { flush: 'sync' })

onMounted(async () => {
  const [pdfjs, lib] = await Promise.all([loadPdfjs(), loadPdfjsViewer()])
  if (unmounted || !container.value || !viewerElement.value) return

  eventBus = new lib.EventBus()
  linkService = new lib.PDFLinkService({
    eventBus,
    externalLinkTarget: lib.LinkTarget.BLANK,
    externalLinkRel: 'noopener noreferrer nofollow',
  })
  findController = new lib.PDFFindController({ linkService, eventBus })
  // Read-only: forms show their values but can't be edited, the annotation
  // editor is off, and with no scripting manager a document's JavaScript
  // never runs. pdf.js caps canvas size itself on iOS and Android.
  viewer = new lib.PDFViewer({
    container: container.value,
    viewer: viewerElement.value,
    eventBus,
    linkService,
    findController,
    textLayerMode: 1,
    annotationMode: pdfjs.AnnotationMode.ENABLE,
    annotationEditorMode: pdfjs.AnnotationEditorType.DISABLE,
    removePageBorders: true,
    imageResourcesPath: pdfjsImagesUrl(pdfjs.version),
    // Typed as pdf.js's own L10n class, whose private fields no adapter can
    // have; pdf.js calls only the methods createPdfjsL10n provides.
    l10n: l10n as unknown as ConstructorParameters<typeof PDFViewer>[0]['l10n'],
  })
  linkService.setViewer(viewer)
  // pdf.js translates its own container only when it made the L10n itself.
  void l10n.translate(container.value)

  eventBus.on('pagesinit', () => {
    if (!viewer) return
    viewer.currentScaleValue = 'auto'
    if (props.initialPage) viewer.currentPageNumber = clampPage(props.initialPage)
    container.value?.focus({ preventScroll: true })
    emit('ready')
  })
  eventBus.on('pagechanging', ({ pageNumber: page }: { pageNumber: number }) => {
    pageNumber.value = page
    pageInput.value = String(page)
  })
  eventBus.on('scalechanging', ({ scale: value, presetValue }: { scale: number; presetValue?: string }) => {
    scale.value = value
    scalePreset.value = presetValue && Number.isNaN(Number(presetValue)) ? presetValue : null
  })
  eventBus.on('updatefindmatchescount', ({ matchesCount }: { matchesCount: FindMatches }) => {
    findMatches.value = matchesCount
  })
  eventBus.on(
    'updatefindcontrolstate',
    ({ state, matchesCount }: { state: number; matchesCount?: FindMatches }) => {
      findPending.value = state === lib.FindState.PENDING
      if (matchesCount) findMatches.value = matchesCount
    },
  )

  attachDocument(pdf.pdfDocument.value)
})

onBeforeUnmount(() => {
  unmounted = true
  answerPassword(null)
  attachDocument(null)
  void l10n.destroy()
})

// ── Navigation ──────────────────────────────────────────────────
const clampPage = (page: number) => Math.min(Math.max(1, Math.round(page)), Math.max(1, pagesCount.value))

const pageInput = ref('1')
// Wide enough for the last page's number, so typing one never scrolls a digit out of view.
const pageInputWidth = computed(() => `calc(${Math.max(3, String(pagesCount.value).length)}ch + 1.5rem)`)
const commitPage = () => {
  const page = Number(pageInput.value)
  if (viewer && Number.isFinite(page) && pagesCount.value > 0) {
    viewer.currentPageNumber = clampPage(page)
  }
  pageInput.value = String(viewer?.currentPageNumber ?? pageNumber.value)
}

// ── Zoom and rotation ───────────────────────────────────────────
const ZOOM_PRESETS = ['auto', 'page-width', 'page-fit', '0.5', '0.75', '1', '1.25', '1.5', '2', '3', '4']
const zoomOptions = computed<DropdownOption<string>[]>(() =>
  ZOOM_PRESETS.map((value) => ({
    value,
    label:
      value === 'auto'
        ? $t('ticket-media-pdf-zoom-auto')
        : value === 'page-width'
          ? $t('ticket-media-pdf-zoom-page-width')
          : value === 'page-fit'
            ? $t('ticket-media-pdf-zoom-page-fit')
            : `${Math.round(Number(value) * 100)}%`,
  })),
)
// A preset when one is in use or the scale matches one, else empty with the
// percentage as the placeholder (after a pinch or Ctrl+wheel).
const zoomValue = computed(() => scalePreset.value ?? ZOOM_PRESETS.find((p) => Number(p) === scale.value) ?? '')
const zoomPercent = computed(() => `${Math.round(scale.value * 100)}%`)

const setZoom = (value: string) => {
  if (viewer && value) viewer.currentScaleValue = value
}
const zoomIn = () => viewer?.increaseScale()
const zoomOut = () => viewer?.decreaseScale()
const rotate = () => {
  if (viewer) viewer.pagesRotation = (viewer.pagesRotation + 90) % 360
}

// Ctrl/Cmd + wheel and a trackpad pinch zoom the document around the pointer
// instead of the whole app. Browsers report a pinch as a wheel event with
// ctrlKey set while no Ctrl key is down; it zooms smoothly. A mouse wheel
// zooms a step per notch, as the toolbar buttons do.
let ctrlDown = false
const trackCtrl = (event: KeyboardEvent) => {
  ctrlDown = event.ctrlKey
}
const releaseCtrl = () => {
  ctrlDown = false
}
// Pixels (or lines, or pages) a wheel moves per notch.
const NOTCH: Record<number, number> = { [WheelEvent.DOM_DELTA_PIXEL]: 100, [WheelEvent.DOM_DELTA_LINE]: 3, [WheelEvent.DOM_DELTA_PAGE]: 1 }
let notches = 0
const onWheel = (event: WheelEvent) => {
  if (!viewer || !(event.ctrlKey || event.metaKey)) return
  event.preventDefault()
  const origin: [number, number] = [event.clientX, event.clientY]
  const pinch =
    event.ctrlKey && !ctrlDown && event.deltaMode === WheelEvent.DOM_DELTA_PIXEL && Math.abs(event.deltaY) < 50
  if (pinch) {
    viewer.updateScale({ scaleFactor: Math.exp(-event.deltaY / 100), origin, drawingDelay: 400 })
    return
  }
  const moved = event.deltaY / (NOTCH[event.deltaMode] ?? NOTCH[WheelEvent.DOM_DELTA_PIXEL])
  // Partial notches add up, until the wheel turns the other way.
  notches = Math.sign(notches) === -Math.sign(moved) ? moved : notches + moved
  const steps = Math.trunc(notches)
  if (!steps) return
  notches -= steps
  viewer.updateScale({ steps: -steps, origin, drawingDelay: 400 })
}

// ── Keyboard ────────────────────────────────────────────────────
// Up/Down, PageUp/PageDown, Home and End scroll the focused container
// natively. These add page steps, zoom and close.
const onKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Escape') {
    emit('close')
    return
  }
  // Cmd/Ctrl+F finds in the document, Cmd/Ctrl+G steps through matches, and
  // Cmd/Ctrl+P prints it, in place of the browser doing so for the app.
  if ((event.ctrlKey || event.metaKey) && !event.altKey) {
    const key = event.key.toLowerCase()
    if (key === 'f' && pagesCount.value) {
      event.preventDefault()
      openFind()
      return
    }
    if (key === 'g' && findOpen.value) {
      event.preventDefault()
      eventBus?.dispatch('find', { ...lastFind, source: null, type: 'again', findPrevious: event.shiftKey })
      return
    }
    if (key === 'p' && canPrint) {
      event.preventDefault()
      void print()
      return
    }
  }
  const target = event.target as HTMLElement | null
  if (target?.closest('input, textarea, select, [contenteditable="true"]')) return
  if (event.altKey) return

  const handled = (() => {
    switch (event.key) {
      case '+':
      case '=':
        zoomIn()
        return true
      case '-':
      case '_':
        zoomOut()
        return true
      case '0':
        setZoom('auto')
        return true
    }
    if (event.ctrlKey || event.metaKey || event.shiftKey) return false
    // The zoom picker keeps focus after a choice; the zoom keys above still
    // work there, but its arrows are its own.
    if (target?.closest('[role="combobox"], [role="listbox"]')) return false
    const scroller = container.value
    const scrollsSideways = !!scroller && scroller.scrollWidth > scroller.clientWidth
    if (event.key === 'ArrowRight' && !scrollsSideways) return viewer?.nextPage() ?? false
    if (event.key === 'ArrowLeft' && !scrollsSideways) return viewer?.previousPage() ?? false
    return false
  })()
  if (handled) event.preventDefault()
}

// ── Links ───────────────────────────────────────────────────────
// In the app a `target="_blank"` link does nothing, so external links go to
// the system browser. Internal links (a table of contents) are `#` links
// pdf.js handles itself.
const onClick = (event: MouseEvent) => {
  if (!isTauriRuntime()) return
  const link = (event.target as HTMLElement | null)?.closest<HTMLAnchorElement>('.annotationLayer a[href]')
  const href = link?.getAttribute('href')
  if (!link || !href || href.startsWith('#')) return
  event.preventDefault()
  void openExternalUrl(link.href)
}

onMounted(() => {
  // Not `@wheel`: it must be non-passive to stop the browser zooming the app.
  container.value?.addEventListener('wheel', onWheel, { passive: false })
  window.addEventListener('keydown', trackCtrl, true)
  window.addEventListener('keyup', trackCtrl, true)
  window.addEventListener('blur', releaseCtrl)
})
onBeforeUnmount(() => {
  container.value?.removeEventListener('wheel', onWheel)
  window.removeEventListener('keydown', trackCtrl, true)
  window.removeEventListener('keyup', trackCtrl, true)
  window.removeEventListener('blur', releaseCtrl)
})

// ── Find ────────────────────────────────────────────────────────
interface FindMatches {
  current: number
  total: number
}
const findOpen = ref(false)
const findMatches = ref<FindMatches>({ current: 0, total: 0 })
const findPending = ref(false)
const findBar = ref<InstanceType<typeof PdfFindBar> | null>(null)
// The last search, for Cmd/Ctrl+G.
let lastFind = { query: '', caseSensitive: false, entireWord: false, highlightAll: true, matchDiacritics: false }

const openFind = () => {
  if (findOpen.value) findBar.value?.focus()
  else findOpen.value = true
}
const find = (request: PdfFindRequest) => {
  lastFind = { ...lastFind, query: request.query, caseSensitive: request.caseSensitive }
  eventBus?.dispatch('find', {
    ...lastFind,
    source: null,
    type: request.again ? 'again' : '',
    findPrevious: request.previous,
  })
}
const closeFind = () => {
  findOpen.value = false
  findMatches.value = { current: 0, total: 0 }
  eventBus?.dispatch('findbarclose', { source: null })
  container.value?.focus({ preventScroll: true })
}

// ── Print ───────────────────────────────────────────────────────
// The app's webview has no print dialog; there, print comes from the share
// sheet.
const canPrint = !isTauriRuntime()
const printing = ref<{ done: number; total: number } | null>(null)
let printAbort: AbortController | null = null
const toast = useToastStore()

const print = async () => {
  const doc = pdf.pdfDocument.value
  if (!doc || printing.value) return
  printAbort = new AbortController()
  printing.value = { done: 0, total: doc.numPages }
  try {
    await printPdfDocument(doc, {
      signal: printAbort.signal,
      onProgress: (done, total) => (printing.value = { done, total }),
    })
  } catch (error) {
    console.error('PDF print failed:', error)
    toast.error($t('ticket-media-pdf-print-failed'))
  } finally {
    printing.value = null
    printAbort = null
  }
}
const cancelPrint = () => printAbort?.abort()

// ── Share (app) ─────────────────────────────────────────────────
// The app's webview can't download or print, so the file goes to the native
// share sheet instead: Save to Files, AirDrop, Mail and Print on iOS, the
// apps that take a PDF on Android.
const inApp = isTauriRuntime()
const sharing = ref(false)
const share = async () => {
  const doc = pdf.pdfDocument.value
  if (!doc || sharing.value) return
  sharing.value = true
  try {
    await shareFile(await doc.getData(), props.filename, 'application/pdf')
  } catch (error) {
    console.error('PDF share failed:', error)
    toast.error($t('ticket-media-pdf-share-failed'))
  } finally {
    sharing.value = false
  }
}
onBeforeUnmount(cancelPrint)

// ── Download progress ───────────────────────────────────────────
const downloadStatus = computed(() => {
  const progress = pdf.progress.value
  if (!progress) return $t('ticket-media-pdf-loading')
  if (progress.total) {
    return $t('ticket-media-pdf-downloading-percent', {
      percent: Math.min(100, Math.floor((progress.loaded / progress.total) * 100)),
    })
  }
  return $t('ticket-media-pdf-downloading-size', { size: (progress.loaded / 1_048_576).toFixed(1) })
})

const pageStatus = computed(() =>
  pagesCount.value ? $t('ticket-media-pdf-page-status', { page: pageNumber.value, total: pagesCount.value }) : '',
)
</script>

<template>
  <div
    class="pdf-viewer flex flex-col h-full w-full overflow-hidden"
    role="region"
    :aria-label="$t('ticket-media-pdf-aria')"
    @keydown="onKeydown"
    @click="onClick"
  >
    <div class="pdfjs-scope relative flex-1 min-h-0 bg-surface-alt">
      <!-- pdf.js requires an absolutely positioned scroll container. -->
      <div
        ref="container"
        class="pdf-scroller overflow-auto focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent"
        style="position: absolute; inset: 0"
        tabindex="0"
        :aria-label="filename"
        data-testid="pdf-scroller"
      >
        <div ref="viewerElement" class="pdfViewer" />
      </div>

      <div
        v-if="pdf.isLoading.value"
        class="absolute inset-0 bg-surface/80 flex items-center justify-center z-20"
      >
        <div class="flex flex-col items-center gap-3">
          <Spinner size="lg" class="text-accent" />
          <span class="text-sm text-secondary" data-testid="pdf-download-status">{{ downloadStatus }}</span>
        </div>
      </div>

      <PdfFindBar
        v-if="findOpen"
        ref="findBar"
        class="absolute top-2 right-2 z-10"
        :matches="findMatches"
        :pending="findPending"
        @find="find"
        @close="closeFind"
      />

      <div
        v-if="printing"
        class="absolute bottom-3 left-1/2 -translate-x-1/2 z-10 flex items-center gap-3 px-3 py-2 rounded-lg border border-default bg-surface shadow-lg text-sm text-secondary"
        role="status"
        data-testid="pdf-print-status"
      >
        <Spinner size="sm" />
        <span>{{ $t('ticket-media-pdf-print-preparing', { done: printing.done, total: printing.total }) }}</span>
        <button type="button" class="text-accent hover:underline" @click="cancelPrint">
          {{ $t('ticket-media-pdf-print-cancel') }}
        </button>
      </div>
    </div>

    <div
      class="flex-shrink-0 bg-surface border-t border-default px-2 sm:px-4 py-2"
      role="toolbar"
      :aria-label="$t('ticket-media-pdf-toolbar')"
    >
      <div class="flex items-center justify-between gap-2 sm:gap-4 max-w-2xl mx-auto">
        <!-- Page -->
        <div class="flex items-center gap-1.5 sm:gap-2 text-sm text-secondary whitespace-nowrap">
          <input
            v-model="pageInput"
            type="text"
            inputmode="numeric"
            class="px-1 sm:px-2 py-1 text-center tabular-nums rounded-md border border-default bg-surface-alt text-primary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
            :style="{ width: pageInputWidth }"
            :aria-label="$t('ticket-media-pdf-page-input')"
            :disabled="!pagesCount"
            data-testid="pdf-page-input"
            @keydown.enter.prevent="commitPage"
            @blur="commitPage"
          />
          <span>{{ $t('ticket-media-pdf-page-total', { total: pagesCount }) }}</span>
        </div>

        <!-- Zoom and rotate -->
        <div class="flex items-center gap-1">
          <IconButton
            icon="zoomOut"
            size="sm"
            :label="$t('ticket-media-pdf-zoom-out')"
            :disabled="!pagesCount"
            @click="zoomOut"
          />
          <BaseDropdown
            :model-value="zoomValue"
            :options="zoomOptions"
            :placeholder="zoomPercent"
            size="sm"
            class="w-32 sm:w-36"
            :aria-label="$t('ticket-media-pdf-zoom')"
            :disabled="!pagesCount"
            @update:model-value="(value) => setZoom(value as string)"
          />
          <IconButton
            icon="zoomIn"
            size="sm"
            :label="$t('ticket-media-pdf-zoom-in')"
            :disabled="!pagesCount"
            @click="zoomIn"
          />
          <!-- Phones turn the device instead; the wrapper carries the
               breakpoint, since IconButton's own display class wins. -->
          <span class="hidden sm:inline-flex">
            <IconButton
              icon="rotate"
              size="sm"
              :label="$t('ticket-media-pdf-rotate')"
              :disabled="!pagesCount"
              @click="rotate"
            />
          </span>
        </div>

        <!-- Find, print, download -->
        <div class="flex items-center gap-1">
          <IconButton
            icon="search"
            size="sm"
            :label="$t('ticket-media-pdf-find')"
            :disabled="!pagesCount"
            @click="openFind"
          />
          <IconButton
            v-if="canPrint"
            icon="print"
            size="sm"
            :label="$t('ticket-media-pdf-print')"
            :disabled="!pagesCount || !!printing"
            @click="print"
          />
          <IconButton
            v-if="inApp"
            icon="upload"
            size="sm"
            :label="$t('ticket-media-pdf-share')"
            :disabled="!pagesCount"
            :loading="sharing"
            data-testid="pdf-share"
            @click="share"
          />
          <a
            v-else
            :href="src"
            target="_blank"
            :download="filename"
            class="p-1.5 rounded text-secondary hover:bg-surface-hover"
            :title="$t('ticket-media-pdf-download')"
            :aria-label="$t('ticket-media-pdf-download')"
          >
            <Icon name="download" />
          </a>
        </div>
      </div>
    </div>

    <span class="sr-only" aria-live="polite" data-testid="pdf-page-status">{{ pageStatus }}</span>

    <PdfPasswordPrompt
      :show="passwordPrompt !== null"
      :filename="filename"
      :reason="passwordPrompt?.reason ?? 'needed'"
      :busy="passwordPrompt?.busy ?? false"
      @submit="answerPassword"
      @cancel="answerPassword(null)"
    />
  </div>
</template>

<style scoped>
/* pdf.js lays the pages out; this gives them room and a card edge in place
   of its image borders (removePageBorders). */
.pdfjs-scope :deep(.pdfViewer) {
  padding-block: 1rem;
}

.pdfjs-scope :deep(.pdfViewer .page) {
  margin: 0 auto 1rem;
  box-shadow: 0 1px 3px rgb(0 0 0 / 0.2);
  border-radius: 2px;
}
</style>
