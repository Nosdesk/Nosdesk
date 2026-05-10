<script setup lang="ts">
/**
 * Ticket activity timeline.
 *
 * Renders the chronological "who did what when" feed for a single
 * ticket, sourced from the `sync_actions` event log via
 * `GET /api/tickets/:id/activity`. Every status change, assignee
 * swap, priority bump, comment, and category move already lands
 * in `sync_actions` (the sync engine's substrate) — this surface
 * just reads from it. No separate audit table.
 *
 * Phrasing: each `event_type` maps to a verb + object phrase
 * through the dictionary in `formatEvent()` below. The ticket-
 * facing payload carries the FULL row after the change (not a
 * diff), so the phrasing is "X set status to In Progress" rather
 * than "X changed status from Open to In Progress" — the latter
 * would require comparing against the previous event, which is
 * deferrable polish.
 *
 * Pagination: cursor-based descending. The first page renders
 * the most-recent events; "Load older activity" fetches the next
 * page. No infinite scroll — the timeline is a reference surface,
 * not a primary scan target, and explicit pagination keeps the
 * hot DOM small.
 */
import { computed, onMounted, ref, watch } from 'vue'
import {
  getTicketActivity,
  type TicketActivityEvent,
} from '@/services/ticketService'
import { useWorkflowStatesStore } from '@/stores/workflowStates'
import { useUsersDirectory } from '@/composables/useUsersDirectory'
import { formatCompactRelativeTime } from '@/utils/dateUtils'
import UserAvatar from '@/components/UserAvatar.vue'
import Spinner from '@/components/common/Spinner.vue'
import Icon from '@/components/common/Icon.vue'

const props = defineProps<{
  ticketId: number
}>()

const events = ref<TicketActivityEvent[]>([])
const nextCursor = ref<number | null>(null)
const loading = ref(false)
const loadingMore = ref(false)
const error = ref<string | null>(null)

const workflowStatesStore = useWorkflowStatesStore()
const { getUserHandle } = useUsersDirectory()

async function loadInitial() {
  loading.value = true
  error.value = null
  try {
    const res = await getTicketActivity(props.ticketId, { limit: 50 })
    events.value = res.events
    nextCursor.value = res.next_cursor
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load activity'
  } finally {
    loading.value = false
  }
}

async function loadMore() {
  if (!nextCursor.value || loadingMore.value) return
  loadingMore.value = true
  try {
    const res = await getTicketActivity(props.ticketId, {
      before: nextCursor.value,
      limit: 50,
    })
    events.value = [...events.value, ...res.events]
    nextCursor.value = res.next_cursor
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load more activity'
  } finally {
    loadingMore.value = false
  }
}

onMounted(loadInitial)

// Reload when the ticket id changes (route change between tickets
// without a full remount). Listening on the prop covers the
// detail-pane navigation pattern.
watch(() => props.ticketId, () => {
  events.value = []
  nextCursor.value = null
  loadInitial()
})

// ---- Phrasing dictionary ---------------------------------------
//
// One function per event_type the backend emits. Each returns
// the trailing phrase that follows the actor's name (e.g.
// "set status to In Progress", "commented on this ticket"). Keep
// this in sync with `repository::*` emit sites — when a new
// event_type ships, add an arm here. Unknown types fall through
// to a generic "made a change" so the timeline never breaks.
//
// Workflow state names come from the workspace store — admins
// configure these per workspace, so we resolve at render rather
// than baking the labels into the dictionary.

interface PhraseContext {
  workflowName: (id: number) => string | null
  userName: (uuid: string) => string | null
}

function phraseFor(ev: TicketActivityEvent, ctx: PhraseContext): string {
  const data = ev.data ?? {}
  switch (ev.event_type) {
    case 'ticket.created':
      return 'created this ticket'
    case 'ticket.deleted':
      return 'deleted this ticket'
    case 'ticket.workflow_state_changed': {
      const id = data.workflow_state_id as number | undefined
      const name = id != null ? ctx.workflowName(id) : null
      return name ? `set status to ${name}` : 'changed status'
    }
    case 'ticket.assignee_changed': {
      const uuid = data.assignee_uuid as string | null | undefined
      // The phrase rendering takes care of looking up the
      // assignee's name via the directory; the verb just signals
      // assignment vs unassignment so the line scans cleanly even
      // before the user data resolves.
      return uuid ? 'reassigned this ticket' : 'unassigned this ticket'
    }
    case 'ticket.priority_changed': {
      const p = data.priority as string | undefined
      return p ? `set priority to ${p}` : 'changed priority'
    }
    case 'ticket.title_changed': {
      const t = data.title as string | undefined
      return t ? `renamed the ticket to "${t}"` : 'renamed the ticket'
    }
    case 'ticket.category_changed':
      return 'changed the category'
    case 'ticket.verification_changed':
      return 'updated verification state'
    case 'ticket.tags_changed': {
      const added = (data.added as number[] | undefined)?.length ?? 0
      const removed = (data.removed as number[] | undefined)?.length ?? 0
      if (added > 0 && removed === 0) {
        return added === 1 ? 'added a tag' : `added ${added} tags`
      }
      if (removed > 0 && added === 0) {
        return removed === 1 ? 'removed a tag' : `removed ${removed} tags`
      }
      return 'updated the tags'
    }
    case 'ticket.resolution_notes_changed':
      return 'updated the resolution notes'
    case 'ticket.watcher_added': {
      const target = data.user_uuid as string | undefined
      const isSelf = !!target && !!ev.actor_uuid && target === ev.actor_uuid
      if (isSelf) {
        return data.auto_added
          ? 'started watching (auto-subscribed on first reply)'
          : 'started watching this ticket'
      }
      if (!target) return 'added a watcher'
      const name = ctx.userName(target)
      return name ? `added ${name} as a watcher` : 'added a watcher'
    }
    case 'ticket.watcher_removed': {
      const target = data.user_uuid as string | undefined
      const isSelf = !!target && !!ev.actor_uuid && target === ev.actor_uuid
      if (isSelf) return 'stopped watching this ticket'
      if (!target) return 'removed a watcher'
      const name = ctx.userName(target)
      return name ? `removed ${name} as a watcher` : 'removed a watcher'
    }
    case 'ticket.updated':
      return 'updated the ticket'
    case 'comment.created':
      return data.is_internal ? 'added an internal note' : 'commented on this ticket'
    case 'comment.deleted':
      return 'deleted a comment'
    default:
      return 'made a change'
  }
}

// Render context — the workflow store is loaded by the app shell;
// `findById` returns undefined until then, in which case we render
// the fallback "changed status" phrasing. Cheap to recompute on
// every event since it's just an array find.
const ctx = computed<PhraseContext>(() => ({
  workflowName: (id: number) => workflowStatesStore.findById(id)?.name ?? null,
  userName: (uuid: string) => getUserHandle(uuid).user.value?.name ?? null,
}))

// Resolve the assignee handle for the assignee_changed events so
// the line can render "X reassigned this ticket to Y" instead of
// just the verb. Memoised at render time — the directory's
// module-cache deduplicates per-uuid.
function assigneeNameFor(ev: TicketActivityEvent): string | null {
  if (ev.event_type !== 'ticket.assignee_changed') return null
  const uuid = (ev.data?.assignee_uuid as string | null | undefined) ?? null
  if (!uuid) return null
  return getUserHandle(uuid).user.value?.name ?? null
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <!-- Section header. Matches the "Comments" / "Devices"
         headers used elsewhere in the right column so the
         timeline reads as a peer surface, not a popover. -->
    <h3 class="text-sm font-medium text-secondary px-1">Activity</h3>

    <div
      v-if="loading"
      class="flex items-center justify-center py-6 text-tertiary"
    >
      <Spinner size="sm" />
    </div>

    <div
      v-else-if="error"
      class="px-3 py-2 text-xs text-status-error bg-status-error-muted rounded"
    >
      {{ error }}
    </div>

    <div
      v-else-if="events.length === 0"
      class="px-3 py-6 text-xs text-tertiary text-center"
    >
      No activity yet.
    </div>

    <ul v-else class="flex flex-col gap-1.5">
      <!-- Each row: actor avatar + actor name + verb phrase +
           relative timestamp. The avatar uses the directory
           composable so it resolves through the same sync engine
           pool the rest of the app reads from. Actor uuid is
           null for system-generated events (background jobs,
           webhooks); fall back to the actor_kind label. -->
      <li
        v-for="ev in events"
        :key="ev.sync_id"
        class="flex items-start gap-2 px-2 py-1.5 rounded hover:bg-surface-hover/40 transition-colors"
      >
        <!-- Avatar slot. UserAvatar handles the loading skeleton
             when the directory hasn't resolved the uuid yet. -->
        <UserAvatar
          v-if="ev.actor_uuid"
          :name="ev.actor_uuid"
          :user-name="getUserHandle(ev.actor_uuid).user.value?.name ?? undefined"
          :avatar="getUserHandle(ev.actor_uuid).user.value?.avatar_thumb || getUserHandle(ev.actor_uuid).user.value?.avatar_url || null"
          size="xs"
          :show-name="false"
          :clickable="true"
          class="mt-0.5"
        />
        <span
          v-else
          class="inline-flex items-center justify-center w-5 h-5 rounded-full bg-surface-alt text-tertiary text-[9px] mt-0.5 shrink-0"
          :title="ev.actor_kind"
        >sys</span>

        <div class="flex-1 min-w-0 text-xs text-secondary leading-relaxed">
          <span class="font-medium text-primary">
            {{
              ev.actor_uuid
                ? (getUserHandle(ev.actor_uuid).user.value?.name ?? 'Someone')
                : (ev.actor_kind === 'system' ? 'System' : ev.actor_kind)
            }}
          </span>
          {{ phraseFor(ev, ctx) }}
          <template v-if="assigneeNameFor(ev)">
            to <span class="font-medium text-primary">{{ assigneeNameFor(ev) }}</span>
          </template>
          <span class="text-tertiary tabular-nums">
            · {{ formatCompactRelativeTime(ev.occurred_at) }}
          </span>
        </div>
      </li>
    </ul>

    <!-- Load-more affordance. Explicit button rather than
         infinite scroll — the timeline is a reference surface,
         not a primary scan target, and explicit pagination keeps
         the hot DOM small for tickets with hundreds of events. -->
    <button
      v-if="nextCursor != null"
      type="button"
      class="text-xs text-tertiary hover:text-primary px-2 py-1.5 rounded hover:bg-surface-hover transition-colors flex items-center justify-center gap-1.5 self-start"
      :disabled="loadingMore"
      @click="loadMore"
    >
      <Spinner v-if="loadingMore" size="xs" />
      <Icon v-else name="history" class="w-3.5 h-3.5" />
      <span>{{ loadingMore ? 'Loading…' : 'Load older activity' }}</span>
    </button>
  </div>
</template>
