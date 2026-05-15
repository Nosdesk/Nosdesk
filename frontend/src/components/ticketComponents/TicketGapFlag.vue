<!--
  TicketGapFlag: ticket sidebar pill showing this ticket is flagged
  as a knowledge gap.

  Renders nothing when the ticket isn't flagged. The "Flag for
  documentation" action lives in the sidebar Actions section and
  is idempotent on the server side, so this component is purely
  the *flagged* state surface — link to the gap + unflag.

  Self-fetching via useTicketFlagState so multiple sidebar
  consumers share one cache entry per ticket. Mutations invalidate
  the same key.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'
import {
  useTicketFlagState,
  useUnflagTicketMutation,
} from '@/composables/useKnowledgeGaps'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const props = defineProps<{
  ticketId: number
}>()

const { gap, isFlagged } = useTicketFlagState(() => props.ticketId)
const unflagMutation = useUnflagTicketMutation()

const isWorking = computed(() => unflagMutation.asyncStatus.value === 'loading')

async function unflag() {
  await unflagMutation.mutateAsync({ ticketId: props.ticketId })
}
</script>

<template>
  <div
    v-if="isFlagged && gap"
    class="rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 flex items-start gap-2 print:hidden"
  >
    <Icon name="warning" class="text-amber-600 dark:text-amber-400 flex-shrink-0 mt-0.5" />
    <div class="flex-1 min-w-0">
      <p class="text-xs font-medium text-amber-800 dark:text-amber-200">
        {{ t('ticket-chip-gap-flagged') }}
      </p>
      <RouterLink
        :to="`/documentation/gaps/${gap.id}`"
        class="text-[11px] text-amber-700 dark:text-amber-300 hover:underline"
      >
        {{ t('ticket-chip-gap-view-queue') }}
      </RouterLink>
    </div>
    <button
      type="button"
      :disabled="isWorking"
      class="flex-shrink-0 text-[11px] text-tertiary hover:text-status-error transition-colors disabled:opacity-50"
      :title="t('ticket-chip-gap-remove-flag')"
      @click="unflag"
    >
      <Icon name="close" size="xs" />
    </button>
  </div>
</template>
