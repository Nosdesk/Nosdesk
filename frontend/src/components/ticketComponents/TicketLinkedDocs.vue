<!--
  TicketLinkedDocs: property-list row for documentation pages
  linked to the ticket. Each linked doc renders as a chip
  pointing at the doc; the page icon (or a default 📄) leads.
  Read-only un/link from this surface; the authoring side lives
  in TicketSaveAsDoc and the doc's own panel.

  Self-fetching via useTicketDocs (Pinia Colada) so the same
  cache entry powers any other widget that surfaces this list.
-->
<script setup lang="ts">
import PropertyChipRow from '@/components/ticketComponents/PropertyChipRow.vue'
import PropertyChip from '@/components/ticketComponents/PropertyChip.vue'
import { useTicketDocs } from '@/composables/usePageTicketLinks'
import { docUrl } from '@/utils/docUrl'

const props = defineProps<{
  ticketId: number
}>()

const emit = defineEmits<{
  (e: 'add'): void
}>()

const { links } = useTicketDocs(() => props.ticketId)
</script>

<template>
  <PropertyChipRow
    label="Documentation"
    add-label="Save as doc"
    @add="emit('add')"
  >
    <PropertyChip
      v-for="link in links"
      :key="link.page_id"
      :label="link.page_title"
      :title="link.link_type === 'resolves' ? `${link.page_title} · resolves this ticket` : link.page_title"
      :to="docUrl({ slug: link.page_slug, id: link.page_id })"
    >
      <template #leading>
        <span class="leading-none">{{ link.page_icon || '📄' }}</span>
      </template>
    </PropertyChip>
  </PropertyChipRow>
</template>
