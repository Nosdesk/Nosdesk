<!--
Persistent banner shown above the ticket details panel when the ticket
is a merge source (merged_into_ticket_id is set). Links through to the
destination. The ticket is terminal: its comments and article are
read-only (gated separately in TicketView).
-->
<script setup lang="ts">
import { useRouter } from 'vue-router'
import Icon from '@/components/common/Icon.vue'

const props = defineProps<{
  targetId: number
  actor?: string | null
  when?: string | null
}>()

const router = useRouter()

function openDestination() {
  router.push(`/tickets/${props.targetId}`)
}
</script>

<template>
  <div
    class="flex items-center gap-3 rounded-lg border border-default bg-surface-alt px-4 py-2.5 text-sm"
    role="status"
  >
    <Icon name="info" class="w-4 h-4 text-tertiary shrink-0" />
    <span class="text-secondary">
      {{
        $t('ticket-merge-banner-merged-into', {
          target_id: targetId,
          actor: actor || '',
          when: when || '',
        })
      }}
    </span>
    <button
      type="button"
      class="ml-auto inline-flex items-center gap-1 px-2 py-1 rounded text-xs text-accent hover:bg-accent/10 transition-colors"
      @click="openDestination"
    >
      {{ $t('ticket-merge-banner-open-destination') }}
    </button>
  </div>
</template>
