<!-- Why a PDF isn't showing, in words, with the next step that helps. -->
<script setup lang="ts">
import { computed } from 'vue'
import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'
import type { PdfFailure } from '@/utils/pdfjs'

/** 'no-source' is a link that names no file the viewer may open. */
export type PdfViewerFailure = PdfFailure | 'no-source'

const props = defineProps<{ failure: PdfViewerFailure }>()

const emit = defineEmits<{
  retry: []
  close: []
}>()

const COPY: Record<PdfViewerFailure, { title: string; body: string; retry?: string }> = {
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
}
const copy = computed(() => COPY[props.failure])
const locked = computed(() => props.failure === 'password-cancelled')
</script>

<template>
  <div class="h-full flex items-center justify-center p-6">
    <div
      role="alert"
      class="bg-surface border border-default p-6 rounded-lg shadow-lg max-w-md flex items-start gap-4"
      data-testid="pdf-viewer-failure"
    >
      <Icon
        :name="locked ? 'lock' : 'warning'"
        size="lg"
        :class="locked ? 'text-secondary flex-shrink-0' : 'text-status-error flex-shrink-0'"
      />
      <div class="flex flex-col gap-2">
        <h3 class="text-lg font-semibold text-primary">{{ $t(copy.title) }}</h3>
        <p class="text-secondary">{{ $t(copy.body) }}</p>
        <div class="flex flex-wrap gap-2 mt-2">
          <Button v-if="copy.retry" @click="emit('retry')">
            {{ $t(copy.retry) }}
          </Button>
          <Button variant="ghost" @click="emit('close')">
            {{ $t('pdf-viewer-close') }}
          </Button>
        </div>
      </div>
    </div>
  </div>
</template>
