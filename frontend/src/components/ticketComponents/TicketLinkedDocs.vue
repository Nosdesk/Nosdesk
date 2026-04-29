<!--
  TicketLinkedDocs: "See also" panel for the ticket sidebar.

  Shows documentation pages currently linked to the ticket via the
  documentation_page_tickets join. Read-only by design here — the
  authoring side of the link (creating a doc from a ticket) lives
  in TicketSaveAsDoc; un/linking arbitrary existing docs to a
  ticket happens from the doc's own panel where the canonical doc
  picker already lives.

  Self-fetching via useTicketDocs (Pinia Colada) so the same cache
  entry powers any other widget that surfaces this list (e.g. a
  future "Recently used in tickets" panel on the doc detail).
-->
<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import Icon from '@/components/common/Icon.vue'
import SidebarSection from '@/components/ticketComponents/SidebarSection.vue'
import { useTicketDocs } from '@/composables/usePageTicketLinks'
import { docUrl } from '@/utils/docUrl'

const props = defineProps<{
  ticketId: number
}>()

const emit = defineEmits<{
  (e: 'add'): void
}>()

const { links, isLoading } = useTicketDocs(() => props.ticketId)

const hasAny = computed(() => links.value.length > 0)
</script>

<template>
  <!--
    Render only when there's content. The SidebarAddMenu carries the
    "Save as doc" action when the section is empty so the sidebar
    isn't a stack of empty headers.
  -->
  <SidebarSection
    v-if="hasAny || isLoading"
    title="Documentation"
    add-label="Save as doc"
    :has-items="hasAny"
    hide-empty-state
    @add="emit('add')"
  >
    <div v-if="isLoading" class="text-xs text-tertiary py-1">Loading&hellip;</div>

    <div v-else class="flex flex-col gap-1">
      <RouterLink
        v-for="link in links"
        :key="link.page_id"
        :to="docUrl({ slug: link.page_slug, id: link.page_id })"
        class="group flex items-center gap-2 py-1.5 px-2 -mx-2 rounded text-xs text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      >
        <span class="flex-shrink-0 text-base leading-none">{{ link.page_icon || '📄' }}</span>
        <span class="flex-1 truncate">{{ link.page_title }}</span>
        <span
          v-if="link.link_type === 'resolves'"
          class="text-[10px] uppercase tracking-wide text-emerald-600 dark:text-emerald-400"
          title="This doc resolved this ticket"
        >
          resolves
        </span>
        <Icon name="link" size="xs" class="text-tertiary opacity-0 group-hover:opacity-100 transition-opacity" />
      </RouterLink>
    </div>
  </SidebarSection>
</template>
