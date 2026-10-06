<!--
Persistent banner shown above the ticket details panel when the ticket
is a merge source (merged_into_ticket_id is set). Links through to the
destination. The ticket is terminal: its comments are read-only (gated
separately in TicketView).
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'
import { numberForTicketId } from '@/composables/useTicketNumberLookup'
import { useUsersDirectory } from '@/composables/useUsersDirectory'
import { formatDateTime } from '@nosdesk/core/utils/dateUtils'
import { ticketPath } from '@/utils/ticketNumbers'

const props = defineProps<{
  targetId: number
  /** The uuid of the person who merged it. */
  actor?: string | null
  when?: string | null
}>()

const fluent = useFluent()
const { getUserHandle } = useUsersDirectory()

// Unknown while it's looked up, or when the viewer can't see the destination.
const targetNumber = computed(() => numberForTicketId(props.targetId))
// The merger's name once the directory has it; the sentence leaves out "by"
// until then, rather than showing a uuid.
const actorName = computed(() =>
  props.actor ? getUserHandle(props.actor).user.value?.name : undefined,
)
const message = computed(() => {
  const when = props.when ? formatDateTime(props.when) : ''
  const key = targetNumber.value !== undefined
    ? 'ticket-merge-banner-merged-into'
    : 'ticket-merge-banner-merged-into-another'
  return actorName.value
    ? fluent.$t(key, { target_id: targetNumber.value ?? '', actor: actorName.value, when })
    : fluent.$t(`${key}-no-actor`, { target_id: targetNumber.value ?? '', when })
})
</script>

<template>
  <div
    class="flex items-center gap-3 rounded-lg border border-default bg-surface-alt px-4 py-2.5 text-sm"
    role="status"
  >
    <Icon name="info" class="w-4 h-4 text-tertiary shrink-0" />
    <span class="text-secondary">{{ message }}</span>
    <RouterLink
      :to="ticketPath({ id: targetId, number: targetNumber })"
      class="ml-auto inline-flex items-center gap-1 px-2 py-1 rounded text-xs text-accent hover:bg-accent/10 transition-colors"
    >
      {{ $t('ticket-merge-banner-open-destination') }}
    </RouterLink>
  </div>
</template>
