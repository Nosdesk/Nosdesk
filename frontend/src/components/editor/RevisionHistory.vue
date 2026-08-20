<script setup lang="ts">
/**
 * Revision history side-sheet. Thin wrapper that puts a
 * `<RevisionList>` inside a `<ResponsivePanel>` (side panel at
 * md+, bottom sheet on mobile). Use this where the surrounding
 * surface doesn't already provide its own card chrome — most
 * notably the documentation reader at `DocumentView.vue`.
 *
 * For contexts that *do* own their chrome (e.g. the Ticket Notes
 * card on TicketView), drop in `<RevisionList>` directly so the
 * list shares the parent's frame instead of stacking a second
 * panel on top.
 */
import { useFluent } from 'fluent-vue'
import RevisionList from './RevisionList.vue'
import ResponsivePanel from '@/components/common/ResponsivePanel.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

interface Props {
  open?: boolean
  ticketId?: number
  documentId?: number
  type?: 'ticket' | 'documentation'
  activeRevisionNumber?: number | null
}

withDefaults(defineProps<Props>(), {
  open: true,
  type: 'ticket',
})

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'selectRevision', revisionNumber: number | null): void
  (e: 'restored', revisionNumber: number): void
}>()
</script>

<template>
  <ResponsivePanel
    :open="open"
    :title="t('editor-revision-history-title')"
    side-panel-class="w-80"
    @close="emit('close')"
  >
    <RevisionList
      :ticket-id="ticketId"
      :document-id="documentId"
      :type="type"
      :active-revision-number="activeRevisionNumber"
      @select-revision="(n) => emit('selectRevision', n)"
      @restored="(n) => emit('restored', n)"
    />
  </ResponsivePanel>
</template>
