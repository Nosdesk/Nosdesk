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

/**
 * Channel-or-portal annotation attached to `ticket.created` events
 * by the backend (`repository::tickets::TicketCreationAnnotation`).
 * Drives both the actor display and the trailing phrase so the
 * activity entry reads as e.g. "alice@example.com opened this
 * ticket via email" rather than the generic "System created this
 * ticket".
 */
interface CreatedVia {
  source: string | null
  from_email: string | null
  from_name: string | null
  subject: string | null
}

/**
 * Extract the channel/portal annotation when present. Both
 * `ticket.created` and `comment.created` events carry the same
 * `created_via` shape (`subject` is only meaningful for the ticket
 * variant), so one parser handles both. Returns null for UI-
 * authored events and for legacy rows ingested before the
 * annotation shipped — the renderer falls back to its old
 * "System" phrasing in those cases.
 */
function readCreatedVia(ev: TicketActivityEvent): CreatedVia | null {
  if (ev.event_type !== 'ticket.created' && ev.event_type !== 'comment.created') {
    return null
  }
  const cv = ev.data?.created_via
  if (!cv || typeof cv !== 'object') return null
  const obj = cv as Record<string, unknown>
  const source = typeof obj.source === 'string' ? obj.source : null
  // The annotation always emits the keys; treat null/empty as
  // "missing" so the renderer can short-circuit to a simpler phrasing.
  if (!source) return null
  return {
    source,
    from_email: typeof obj.from_email === 'string' ? obj.from_email : null,
    from_name: typeof obj.from_name === 'string' ? obj.from_name : null,
    subject: typeof obj.subject === 'string' ? obj.subject : null,
  }
}

/**
 * Human-readable label for the channel/portal `source` tag. Returns
 * `null` when the source isn't one the renderer knows about — the
 * caller treats that as "use generic phrasing." New channels only
 * need an entry here; the backend tag is conventionally
 * `channel:<provider>`.
 */
function creationSourceLabel(source: string): string | null {
  if (source.startsWith('channel:email')) return 'email'
  if (source.startsWith('channel:slack')) return 'Slack'
  if (source.startsWith('channel:teams')) return 'Microsoft Teams'
  if (source.startsWith('channel:discord')) return 'Discord'
  if (source.startsWith('channel:')) {
    // Generic fallback: turn "channel:custom_provider" into
    // "custom provider" so a new adapter renders meaningfully
    // before someone adds a bespoke label here.
    return source.slice('channel:'.length).replace(/_/g, ' ')
  }
  if (source === 'guest_portal') return 'the public portal'
  return null
}

function phraseFor(ev: TicketActivityEvent, ctx: PhraseContext): string {
  const data = ev.data ?? {}
  switch (ev.event_type) {
    case 'ticket.created': {
      const cv = readCreatedVia(ev)
      if (!cv) return 'created this ticket'
      const label = creationSourceLabel(cv.source!)
      if (!label) return 'created this ticket'
      // Email/chat: "opened... via email". Portal: "submitted...
      // via the public portal". The two verbs match how the agent
      // would think about each surface — emails get opened,
      // portal forms get submitted.
      return cv.source === 'guest_portal'
        ? `submitted this ticket via ${label}`
        : `opened this ticket via ${label}`
    }
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
    case 'comment.created': {
      if (data.is_internal) return 'added an internal note'
      const cv = readCreatedVia(ev)
      const label = cv?.source ? creationSourceLabel(cv.source) : null
      if (!cv || !label) return 'commented on this ticket'
      // Email channels: "replied via email" — the activity is a
      // genuine response, not a fresh comment, when it threads onto
      // an existing ticket. Portal: "added a comment via the
      // public portal" — kept distinct from "submitted this ticket
      // via the public portal" (the ticket.created entry) so the
      // two rows don't read identically.
      return cv.source === 'guest_portal'
        ? `added a comment via ${label}`
        : `replied via ${label}`
    }
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

function isInternalNoteEvent(ev: TicketActivityEvent): boolean {
  return ev.event_type === 'comment.created' && ev.data?.is_internal === true
}

/**
 * Resolve who to show as the actor for an activity row, with a
 * fallback chain that prefers the most-meaningful identity
 * available:
 *
 *   1. Channel/portal annotation (`from_name`/`from_email`) — the
 *      ticket's *sender*, surfaced in place of the system actor so
 *      the agent doesn't have to open the comment thread to see
 *      who reported the issue.
 *   2. The signed-in user who triggered the event
 *      (`actor_uuid`) — resolves via the directory.
 *   3. The bare actor_kind label — the original "System" fallback.
 *
 * The `kind` field drives the avatar slot in the template: real
 * users get the normal avatar; channel/portal senders get a small
 * "@" or "form" badge; pure system actors keep the "sys" badge.
 */
interface ActorDisplay {
  name: string
  /** Tooltip text — full name + email for senders, kind label otherwise. */
  title: string | null
  kind: 'user' | 'email' | 'portal' | 'system'
  /** Set when `kind === 'user'`; drives `<UserAvatar :name="...">`. */
  userUuid: string | null
}

function actorFor(ev: TicketActivityEvent): ActorDisplay {
  const cv = readCreatedVia(ev)
  if (cv && (cv.from_name || cv.from_email)) {
    const name = cv.from_name?.trim() || cv.from_email || 'Sender'
    const title = cv.from_name && cv.from_email
      ? `${cv.from_name} <${cv.from_email}>${cv.subject ? ` — Subject: ${cv.subject}` : ''}`
      : (cv.subject ? `${name} — Subject: ${cv.subject}` : name)
    return {
      name,
      title,
      kind: cv.source === 'guest_portal' ? 'portal' : 'email',
      userUuid: null,
    }
  }
  if (ev.actor_uuid) {
    return {
      name: getUserHandle(ev.actor_uuid).user.value?.name ?? 'Someone',
      title: null,
      kind: 'user',
      userUuid: ev.actor_uuid,
    }
  }
  return {
    name: ev.actor_kind === 'system' ? 'System' : ev.actor_kind,
    title: ev.actor_ref ?? ev.actor_kind,
    kind: 'system',
    userUuid: null,
  }
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
        class="flex items-start gap-2 px-2 py-1.5 rounded transition-colors"
        :class="
          isInternalNoteEvent(ev)
            ? 'bg-status-warning-bg/25 border-l-2 border-status-warning-border/60'
            : 'hover:bg-surface-hover/40'
        "
      >
        <!-- Avatar / origin slot. `actorFor` picks the most
             meaningful identity available: ticket sender for
             channel/portal events, signed-in user for everything
             else, "sys" badge as final fallback. Avatar lookups
             only fire for the user path; the small badges for
             email/portal/system are static so they don't churn the
             directory composable. -->
        <template v-if="actorFor(ev).kind === 'user' && actorFor(ev).userUuid">
          <UserAvatar
            :name="actorFor(ev).userUuid!"
            :user-name="getUserHandle(actorFor(ev).userUuid!).user.value?.name ?? undefined"
            :avatar="getUserHandle(actorFor(ev).userUuid!).user.value?.avatar_thumb || getUserHandle(actorFor(ev).userUuid!).user.value?.avatar_url || null"
            size="xs"
            :show-name="false"
            :clickable="true"
            class="mt-0.5"
          />
        </template>
        <span
          v-else-if="actorFor(ev).kind === 'email'"
          class="inline-flex items-center justify-center w-5 h-5 rounded-full bg-status-info-bg text-status-info-border text-[10px] mt-0.5 shrink-0"
          :title="actorFor(ev).title ?? undefined"
          aria-label="Email sender"
        >@</span>
        <span
          v-else-if="actorFor(ev).kind === 'portal'"
          class="inline-flex items-center justify-center w-5 h-5 rounded-full bg-surface-alt text-tertiary text-[9px] mt-0.5 shrink-0"
          :title="actorFor(ev).title ?? undefined"
          aria-label="Public portal submission"
        >www</span>
        <span
          v-else
          class="inline-flex items-center justify-center w-5 h-5 rounded-full bg-surface-alt text-tertiary text-[9px] mt-0.5 shrink-0"
          :title="actorFor(ev).title ?? undefined"
        >sys</span>

        <div class="flex-1 min-w-0 text-xs text-secondary leading-relaxed">
          <span
            class="font-medium text-primary"
            :title="actorFor(ev).title ?? undefined"
          >{{ actorFor(ev).name }}</span>
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
