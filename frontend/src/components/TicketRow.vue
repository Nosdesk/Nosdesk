<!--
Shared ticket-list row. Used by every dashboard widget that renders a
list of tickets (Assigned Tickets, Recently Viewed, Unassigned Queue)
so the row anatomy stays identical across them: priority rail on the
left, progress-state icon, ticket ID with optional new-activity dot,
title with inline requester name, avatar, and compact relative time.

Fields are flagged optional for shapes that don't carry them (e.g.
`RecentTicket` doesn't include priority or full requester info). The
row degrades gracefully — missing fields simply aren't rendered.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { formatCompactRelativeTime } from '@/utils/dateUtils'
import UserAvatar from './UserAvatar.vue'
import TicketStatusIcon from './TicketStatusIcon.vue'
import type { UserInfo } from '@/types/user'

const props = defineProps<{
  id: number
  title: string
  status: string
  /** Optional — rows sourced from lightweight responses (e.g. recent
   *  views) may not carry priority. The left rail falls back to
   *  transparent in that case, preserving row anatomy. */
  priority?: string
  /** ISO timestamp rendered as compact relative time on the right. */
  timestamp: string
  /** Optional — lightweight response shapes may omit requester
   *  details. When present, the row surfaces name + avatar. */
  requester?: UserInfo | null
  /** True when the ticket has changed since the user last viewed it.
   *  Bolds the title and shows an accent dot before the ID. */
  newActivity?: boolean
  /** Router destination for the row. */
  to: string
}>()

function priorityBarClass(priority?: string): string {
  switch (priority) {
    case 'critical': return 'bg-status-error'
    case 'high': return 'bg-priority-high'
    case 'medium': return 'bg-priority-medium'
    case 'low': return 'bg-priority-low/60'
    default: return 'bg-transparent'
  }
}

const priorityLabel = computed(() =>
  props.priority
    ? `${props.priority.charAt(0).toUpperCase()}${props.priority.slice(1)} priority`
    : '',
)

const statusLabel = computed(() =>
  props.status === 'in-progress'
    ? 'In progress'
    : props.status.charAt(0).toUpperCase() + props.status.slice(1),
)

const ariaLabel = computed(() => {
  const parts = [`Ticket #${props.id}: ${props.title}`]
  if (props.priority) parts.push(`${props.priority} priority`)
  parts.push(statusLabel.value)
  return parts.join(', ')
})
</script>

<template>
  <router-link
    :to="to"
    :aria-label="ariaLabel"
    class="relative group block hover:bg-surface-hover transition-colors"
  >
    <!-- Priority rail — ambient color at the left edge. Always
         rendered so row anatomy is constant across widgets; when
         priority is unknown the rail is transparent. -->
    <span
      aria-hidden="true"
      class="absolute left-0 top-0 bottom-0 w-[3px]"
      :class="priorityBarClass(priority)"
      :title="priorityLabel || undefined"
    />
    <div class="flex items-center gap-2.5 pl-4 pr-3 h-10 min-w-0">
      <TicketStatusIcon :status="status" class="w-3.5 h-3.5" />

      <span class="flex items-center gap-1.5 flex-shrink-0 font-mono text-[11px] text-tertiary tabular-nums">
        <span
          v-if="newActivity"
          class="w-1.5 h-1.5 rounded-full bg-accent flex-shrink-0"
          title="New activity since you last viewed this"
          aria-label="New activity"
        />
        <span>#{{ id }}</span>
      </span>

      <div class="flex-1 min-w-0 flex items-baseline gap-2">
        <h3
          class="text-[13px] truncate min-w-0 group-hover:text-accent transition-colors"
          :class="[
            newActivity ? 'font-semibold' : 'font-medium',
            status === 'closed' ? 'text-tertiary' : 'text-primary',
          ]"
        >
          {{ title }}
        </h3>
        <span
          v-if="requester"
          class="text-[11px] text-tertiary truncate flex-shrink min-w-0 whitespace-nowrap"
          :title="`From ${requester.name}`"
        >
          {{ requester.name }}
        </span>
      </div>

      <UserAvatar
        v-if="requester"
        :name="requester.name"
        :avatar="requester.avatar_thumb"
        :userUuid="requester.uuid"
        size="xxs"
        :showName="false"
        :title="`From ${requester.name}`"
      />
      <span
        class="text-[11px] text-tertiary tabular-nums flex-shrink-0 min-w-[1.75rem] text-right"
        :title="new Date(timestamp).toLocaleString()"
      >
        {{ formatCompactRelativeTime(timestamp) }}
      </span>
    </div>
  </router-link>
</template>
