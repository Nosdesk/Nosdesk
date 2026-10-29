<!--
PdfOverlay — the single host for the PDF viewer.

Mounted once at app root. Shows the file `usePdfOverlay` names (a `?pdf=`
query on the current route, or an unsaved file opened from memory) in a
full-screen dialog over the page. Reka owns the dialog behaviour: focus
trapped inside and returned to the opener on close, the page behind hidden
from assistive tech, Escape to close.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { DialogContent, DialogOverlay, DialogPortal, DialogRoot } from 'reka-ui'
import PdfOverlayBody from '@/components/ticketComponents/PdfOverlayBody.vue'
import { usePdfOverlay } from '@/composables/usePdfOverlay'

const router = useRouter()
const route = useRoute()
const overlay = usePdfOverlay(router, route)

// Also open for a `?pdf=` the overlay may not show, to say so rather than
// ignore the link.
const open = computed(() => overlay.source.value !== null || route.query.pdf !== undefined)

const onOpenChange = (value: boolean) => {
  if (!value) overlay.close()
}
</script>

<template>
  <DialogRoot :open="open" @update:open="onOpenChange">
    <DialogPortal>
      <DialogOverlay class="fixed inset-0 z-overlay bg-black/60" />
      <DialogContent
        class="fixed inset-0 z-overlay flex flex-col bg-app focus:outline-none"
        :aria-describedby="undefined"
        data-testid="pdf-overlay"
      >
        <PdfOverlayBody :source="overlay.source.value" @close="overlay.close" />
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
