<!--
  PageTicketLinksPanel: tickets the page resolves or references.

  The panel is self-fetching — it owns a Pinia Colada query keyed
  on the page id, so the doc detail view doesn't need to thread
  link data through props. The same key is invalidated by the
  link/unlink mutations, so adding a ticket from this panel and
  refreshing happens through one cache update.

  Read-only when `canEdit` is false; technicians get the inline
  "Link a ticket..." affordance which opens the shared
  TicketPickerModal (the app-wide ticket picker).
-->
<script setup lang="ts">
import { computed, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'
import TicketPickerModal from '@/components/ticketComponents/TicketPickerModal.vue'
import { coarseStatusBucket, type WorkflowStateCategory } from '@nosdesk/core/types/workflow'

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)
import {
  usePageTickets,
  useLinkTicketMutation,
  useUnlinkTicketMutation,
} from '@/composables/usePageTicketLinks'

const props = defineProps<{
  pageId: string | number
  /** When true, render the add/remove affordances. */
  canEdit?: boolean
}>()

const { links, isLoading } = usePageTickets(() => props.pageId)
const linkMutation = useLinkTicketMutation()
const unlinkMutation = useUnlinkTicketMutation()

const showPicker = ref(false)
// Existing ticket-picker filters out tickets already linked. The
// modal expects a "current ticket" id; for the doc context we
// pass 0 to opt out of self-exclusion.
const existingTicketIds = computed(() => links.value.map((l) => l.ticket_id))

/** Group links by type. 'resolves' is the strong relationship —
 *  this doc was the answer — so it gets headline framing.
 *  'references' is supporting context. */
const grouped = computed(() => {
  const resolves = links.value.filter((l) => l.link_type === 'resolves')
  const references = links.value.filter((l) => l.link_type === 'references')
  return { resolves, references }
})

const hasAny = computed(() => links.value.length > 0)

async function onPickTicket(ticketId: number) {
  showPicker.value = false
  await linkMutation.mutateAsync({
    pageId: props.pageId,
    ticketId,
    linkType: 'references',
  })
}

async function onRemove(ticketId: number) {
  await unlinkMutation.mutateAsync({ pageId: props.pageId, ticketId })
}

function categoryClass(category: WorkflowStateCategory | null | undefined): string {
  if (!category) return 'bg-surface-alt text-tertiary'
  switch (coarseStatusBucket(category)) {
    case 'open':
      return 'bg-blue-500/10 text-blue-700 dark:text-blue-300'
    case 'in-progress':
      return 'bg-amber-500/10 text-amber-700 dark:text-amber-300'
    case 'closed':
      return 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-300'
    default:
      return 'bg-surface-alt text-tertiary'
  }
}

function categoryLabel(category: WorkflowStateCategory | null | undefined): string {
  if (!category) return ''
  switch (coarseStatusBucket(category)) {
    case 'open':
      return t('status-open')
    case 'in-progress':
      return t('status-in-progress')
    default:
      return t('status-closed')
  }
}
</script>

<template>
  <section class="flex flex-col gap-2">
    <header class="flex items-center justify-between gap-2 pb-2 border-b border-default">
      <div class="flex items-center gap-2">
        <Icon name="ticket" class="text-tertiary" />
        <h3 class="text-xs font-semibold uppercase tracking-wide text-tertiary">
          {{ $t('docs-page-tickets-heading') }}
        </h3>
        <span v-if="hasAny" class="text-[11px] text-tertiary">
          {{ links.length }}
        </span>
      </div>
      <button
        v-if="canEdit"
        type="button"
        class="text-xs px-2 py-1 rounded-md text-secondary hover:text-primary hover:bg-surface-hover transition-colors flex items-center gap-1"
        @click="showPicker = true"
      >
        <Icon name="add" />
        {{ $t('docs-page-tickets-add') }}
      </button>
    </header>

    <!-- Loading: simple text since the section is small. -->
    <p v-if="isLoading" class="text-xs text-tertiary py-1">{{ $t('docs-page-tickets-loading') }}</p>

    <p v-else-if="!hasAny" class="text-xs text-tertiary py-1">
      {{ $t('docs-page-tickets-empty') }}
    </p>

    <template v-else>
      <!-- Resolved group: tickets this doc answered. -->
      <div v-if="grouped.resolves.length > 0" class="flex flex-col gap-1">
        <p class="text-[10px] uppercase tracking-wide text-tertiary mt-1">
          {{ $t('docs-page-tickets-resolved-heading') }}
        </p>
        <ul class="flex flex-col gap-0.5">
          <li v-for="link in grouped.resolves" :key="link.ticket_id" class="group flex items-center gap-2">
            <RouterLink
              :to="`/tickets/${link.ticket_id}`"
              class="flex-1 min-w-0 flex items-center gap-2 py-1 px-2 -mx-2 rounded text-xs text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
            >
              <span class="text-tertiary flex-shrink-0">#{{ link.ticket_id }}</span>
              <span class="flex-1 truncate">{{ link.ticket_title || $t('docs-page-tickets-fallback-title', { id: link.ticket_id }) }}</span>
              <span
                v-if="link.ticket_category"
                class="text-[10px] px-1.5 py-0.5 rounded-full"
                :class="categoryClass(link.ticket_category)"
              >
                {{ categoryLabel(link.ticket_category) }}
              </span>
            </RouterLink>
            <button
              v-if="canEdit"
              type="button"
              class="opacity-0 group-hover:opacity-100 p-1 rounded text-tertiary hover:text-status-error hover:bg-surface-hover transition-all"
              :title="$t('docs-page-tickets-unlink', { id: link.ticket_id })"
              @click="onRemove(link.ticket_id)"
            >
              <Icon name="close" size="xs" />
            </button>
          </li>
        </ul>
      </div>

      <!-- References group: tickets that just point at this doc. -->
      <div v-if="grouped.references.length > 0" class="flex flex-col gap-1">
        <p class="text-[10px] uppercase tracking-wide text-tertiary mt-1">
          {{ $t('docs-page-tickets-referenced-heading') }}
        </p>
        <ul class="flex flex-col gap-0.5">
          <li v-for="link in grouped.references" :key="link.ticket_id" class="group flex items-center gap-2">
            <RouterLink
              :to="`/tickets/${link.ticket_id}`"
              class="flex-1 min-w-0 flex items-center gap-2 py-1 px-2 -mx-2 rounded text-xs text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
            >
              <span class="text-tertiary flex-shrink-0">#{{ link.ticket_id }}</span>
              <span class="flex-1 truncate">{{ link.ticket_title || $t('docs-page-tickets-fallback-title', { id: link.ticket_id }) }}</span>
              <span
                v-if="link.ticket_category"
                class="text-[10px] px-1.5 py-0.5 rounded-full"
                :class="categoryClass(link.ticket_category)"
              >
                {{ categoryLabel(link.ticket_category) }}
              </span>
            </RouterLink>
            <button
              v-if="canEdit"
              type="button"
              class="opacity-0 group-hover:opacity-100 p-1 rounded text-tertiary hover:text-status-error hover:bg-surface-hover transition-all"
              :title="$t('docs-page-tickets-unlink', { id: link.ticket_id })"
              @click="onRemove(link.ticket_id)"
            >
              <Icon name="close" size="xs" />
            </button>
          </li>
        </ul>
      </div>
    </template>

    <TicketPickerModal
      :show="showPicker"
      :exclude-ids="existingTicketIds"
      @close="showPicker = false"
      @select="(t) => onPickTicket(t.id)"
    />
  </section>
</template>
