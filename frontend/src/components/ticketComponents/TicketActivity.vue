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
import { useFluent } from 'fluent-vue'
import {
  getTicketActivity,
  type TicketActivityEvent,
} from '@/services/ticketService'
import { useWorkflowStatesStore } from '@/stores/workflowStates'
import { useUsersDirectory } from '@/composables/useUsersDirectory'
import { useTicketActivitySSE } from '@/composables/useTicketActivitySSE'
import { formatCompactRelativeTime } from '@/utils/dateUtils'
import UserAvatar from '@/components/UserAvatar.vue'
import Spinner from '@/components/common/Spinner.vue'
import Icon from '@/components/common/Icon.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

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
    error.value = e instanceof Error ? e.message : t('ticket-activity-load-error')
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
    error.value = e instanceof Error ? e.message : t('ticket-activity-load-more-error')
  } finally {
    loadingMore.value = false
  }
}

// Pull in activity that landed since our newest entry and prepend it.
// Driven by SSE (useTicketActivitySSE) so the feed stays live for
// collaborators' changes without polling. Dedupes by sync_id in case a
// live event races an in-flight load. Best-effort: a failure is
// swallowed; the next event or a remount recovers.
async function refreshNewest() {
  const headId = events.value[0]?.sync_id
  if (headId == null) {
    // Nothing loaded yet: a normal initial load covers it.
    await loadInitial()
    return
  }
  try {
    const res = await getTicketActivity(props.ticketId, { after: headId, limit: 50 })
    if (res.events.length === 0) return
    const known = new Set(events.value.map((e) => e.sync_id))
    const fresh = res.events.filter((e) => !known.has(e.sync_id))
    if (fresh.length > 0) {
      // Response is newest-first; prepend preserves overall ordering
      // and the grouping recomputes over the merged list.
      events.value = [...fresh, ...events.value]
    }
  } catch {
    // ignore — live refresh is best-effort
  }
}

// Live-refresh on collaborators' changes. The connection itself is
// owned by useTicketSSE at the TicketView level; this only adds
// listeners.
useTicketActivitySSE(
  computed(() => props.ticketId),
  refreshNewest,
)

onMounted(loadInitial)

// Reload when the ticket id changes (route change between tickets
// without a full remount). Listening on the prop covers the
// detail-pane navigation pattern.
watch(() => props.ticketId, () => {
  events.value = []
  nextCursor.value = null
  expanded.value = new Set()
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
  if (source.startsWith('channel:email')) return t('ticket-activity-channel-email')
  if (source.startsWith('channel:slack')) return t('ticket-activity-channel-slack')
  if (source.startsWith('channel:teams')) return t('ticket-activity-channel-teams')
  if (source.startsWith('channel:discord')) return t('ticket-activity-channel-discord')
  if (source.startsWith('channel:')) {
    // Generic fallback: turn "channel:custom_provider" into
    // "custom provider" so a new adapter renders meaningfully
    // before someone adds a bespoke label here.
    return source.slice('channel:'.length).replace(/_/g, ' ')
  }
  if (source === 'guest_portal') return t('ticket-activity-actor-portal-label')
  return null
}

function phraseFor(ev: TicketActivityEvent, ctx: PhraseContext): string {
  const data = ev.data ?? {}
  switch (ev.event_type) {
    case 'ticket.created': {
      const cv = readCreatedVia(ev)
      if (!cv) return t('ticket-activity-phrase-created')
      const label = creationSourceLabel(cv.source!)
      if (!label) return t('ticket-activity-phrase-created')
      // Email/chat: "opened... via email". Portal: "submitted...
      // via the public portal". The two verbs match how the agent
      // would think about each surface, emails get opened, portal
      // forms get submitted.
      return cv.source === 'guest_portal'
        ? t('ticket-activity-phrase-submitted-via', { channel: label })
        : t('ticket-activity-phrase-opened-via', { channel: label })
    }
    case 'ticket.deleted':
      return t('ticket-activity-phrase-deleted')
    case 'ticket.workflow_state_changed': {
      const id = data.workflow_state_id as number | undefined
      const name = id != null ? ctx.workflowName(id) : null
      return name
        ? t('ticket-activity-phrase-status-set', { name })
        : t('ticket-activity-phrase-status-changed')
    }
    case 'ticket.assignee_changed': {
      const uuid = data.assignee_uuid as string | null | undefined
      // The phrase rendering takes care of looking up the
      // assignee's name via the directory; the verb just signals
      // assignment vs unassignment so the line scans cleanly even
      // before the user data resolves.
      return uuid
        ? t('ticket-activity-phrase-reassigned')
        : t('ticket-activity-phrase-unassigned')
    }
    case 'ticket.priority_changed': {
      const p = data.priority as string | undefined
      return p
        ? t('ticket-activity-phrase-priority-set', { priority: p })
        : t('ticket-activity-phrase-priority-changed')
    }
    case 'ticket.title_changed': {
      const titleVal = data.title as string | undefined
      return titleVal
        ? t('ticket-activity-phrase-renamed', { title: titleVal })
        : t('ticket-activity-phrase-renamed-plain')
    }
    case 'ticket.category_changed':
      return t('ticket-activity-phrase-category-changed')
    case 'ticket.verification_changed':
      return t('ticket-activity-phrase-verification-changed')
    case 'ticket.tags_changed': {
      const added = (data.added as number[] | undefined)?.length ?? 0
      const removed = (data.removed as number[] | undefined)?.length ?? 0
      if (added > 0 && removed === 0) {
        return t('ticket-activity-phrase-tags-added', { count: added })
      }
      if (removed > 0 && added === 0) {
        return t('ticket-activity-phrase-tags-removed', { count: removed })
      }
      return t('ticket-activity-phrase-tags-updated')
    }
    case 'ticket.resolution_notes_changed':
      return t('ticket-activity-phrase-resolution-changed')
    case 'ticket.watcher_added': {
      const target = data.user_uuid as string | undefined
      const isSelf = !!target && !!ev.actor_uuid && target === ev.actor_uuid
      if (isSelf) {
        return data.auto_added
          ? t('ticket-activity-phrase-watcher-self-auto')
          : t('ticket-activity-phrase-watcher-self-start')
      }
      if (!target) return t('ticket-activity-phrase-watcher-added')
      const name = ctx.userName(target)
      return name
        ? t('ticket-activity-phrase-watcher-added-named', { name })
        : t('ticket-activity-phrase-watcher-added')
    }
    case 'ticket.watcher_removed': {
      const target = data.user_uuid as string | undefined
      const isSelf = !!target && !!ev.actor_uuid && target === ev.actor_uuid
      if (isSelf) return t('ticket-activity-phrase-watcher-self-stop')
      if (!target) return t('ticket-activity-phrase-watcher-removed')
      const name = ctx.userName(target)
      return name
        ? t('ticket-activity-phrase-watcher-removed-named', { name })
        : t('ticket-activity-phrase-watcher-removed')
    }
    case 'ticket.updated':
      return t('ticket-activity-phrase-updated')
    case 'comment.created': {
      if (data.is_internal) return t('ticket-activity-phrase-internal-note')
      const cv = readCreatedVia(ev)
      const label = cv?.source ? creationSourceLabel(cv.source) : null
      if (!cv || !label) return t('ticket-activity-phrase-commented')
      // Email channels: "replied via email" - the activity is a
      // genuine response, not a fresh comment, when it threads onto
      // an existing ticket. Portal: "added a comment via the public
      // portal", kept distinct from the ticket.created entry so the
      // two rows don't read identically.
      return cv.source === 'guest_portal'
        ? t('ticket-activity-phrase-comment-via', { channel: label })
        : t('ticket-activity-phrase-replied-via', { channel: label })
    }
    case 'comment.deleted':
      return t('ticket-activity-phrase-comment-deleted')
    default:
      return t('ticket-activity-phrase-generic')
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
  /** Set when `kind === 'user'`; drives `<UserAvatar :uuid="...">`. */
  userUuid: string | null
}

function actorFor(ev: TicketActivityEvent): ActorDisplay {
  const cv = readCreatedVia(ev)
  if (cv && (cv.from_name || cv.from_email)) {
    const name = cv.from_name?.trim() || cv.from_email || t('ticket-activity-actor-sender')
    let title: string
    if (cv.from_name && cv.from_email) {
      title = cv.subject
        ? t('ticket-activity-actor-title-named-subject', { name: cv.from_name, email: cv.from_email, subject: cv.subject })
        : t('ticket-activity-actor-title-named', { name: cv.from_name, email: cv.from_email })
    } else {
      title = cv.subject
        ? t('ticket-activity-actor-title-subject', { name, subject: cv.subject })
        : name
    }
    return {
      name,
      title,
      kind: cv.source === 'guest_portal' ? 'portal' : 'email',
      userUuid: null,
    }
  }
  if (ev.actor_uuid) {
    return {
      name: getUserHandle(ev.actor_uuid).user.value?.name ?? t('ticket-activity-actor-someone'),
      title: null,
      kind: 'user',
      userUuid: ev.actor_uuid,
    }
  }
  return {
    name: ev.actor_kind === 'system' ? t('ticket-activity-actor-system') : ev.actor_kind,
    title: ev.actor_ref ?? ev.actor_kind,
    kind: 'system',
    userUuid: null,
  }
}

// ---- Grouping ---------------------------------------------------
//
// Consecutive low-signal field changes by the same actor within a
// short window collapse into one "made N changes" row, expandable on
// click. The window models "one editing session": the backend writes
// one event per update request, so the noise we're collapsing is a
// burst of separate edits by the same person. Comments and ticket
// creation/deletion are milestones and never fold in — they keep
// their own line. Grouping is purely a render concern over the
// already-fetched events; pagination and payloads are untouched.

const GROUP_WINDOW_MS = 10 * 60 * 1000 // 10 minutes — one editing session

function isGroupable(ev: TicketActivityEvent): boolean {
  return (
    ev.event_type.startsWith('ticket.') &&
    ev.event_type !== 'ticket.created' &&
    ev.event_type !== 'ticket.deleted'
  )
}

// Stable "same actor" key for run detection. Groupable events are
// always agent/system actions, so uuid + kind fully identifies them.
function actorKey(ev: TicketActivityEvent): string {
  return `${ev.actor_kind}:${ev.actor_uuid ?? ''}`
}

// One shape for both row kinds (a single row carries a one-element
// events array) so the template never narrows a union — keeps Volar
// happy and field access uniform.
interface TimelineItem {
  kind: 'single' | 'bundle'
  key: string
  anchorId: number
  events: TicketActivityEvent[]
}

const timeline = computed<TimelineItem[]>(() => {
  const list = events.value
  const items: TimelineItem[] = []
  let i = 0
  while (i < list.length) {
    const ev = list[i]
    if (isGroupable(ev)) {
      // events are newest-first; chain older same-actor changes that
      // fall within the window of this anchor (the newest in the run).
      const anchorMs = new Date(ev.occurred_at).getTime()
      const key = actorKey(ev)
      const run = [ev]
      let j = i + 1
      while (
        j < list.length &&
        isGroupable(list[j]) &&
        actorKey(list[j]) === key &&
        anchorMs - new Date(list[j].occurred_at).getTime() <= GROUP_WINDOW_MS
      ) {
        run.push(list[j])
        j += 1
      }
      if (run.length >= 2) {
        items.push({ kind: 'bundle', key: `b${ev.sync_id}`, anchorId: ev.sync_id, events: run })
      } else {
        items.push({ kind: 'single', key: `s${ev.sync_id}`, anchorId: ev.sync_id, events: [ev] })
      }
      i = j
    } else {
      items.push({ kind: 'single', key: `s${ev.sync_id}`, anchorId: ev.sync_id, events: [ev] })
      i += 1
    }
  }
  return items
})

// Expanded bundles, keyed by anchor sync_id. Collapsed by default —
// shrinking the feed is the whole point.
const expanded = ref<Set<number>>(new Set())
function toggleBundle(id: number) {
  const next = new Set(expanded.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  expanded.value = next
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <!-- Section header. Matches the "Comments" / "Devices"
         headers used elsewhere in the right column so the
         timeline reads as a peer surface, not a popover. -->
    <h3 class="text-sm font-medium text-secondary px-1">{{ t('ticket-activity-section-title') }}</h3>

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
      {{ t('ticket-activity-empty') }}
    </div>

    <ul v-else class="flex flex-col gap-1.5">
      <!-- Each item is either a standalone event (comment, ticket
           creation, or a lone change) or a collapsed run of
           consecutive field changes by one actor ("made N changes").
           Bundles default collapsed; clicking expands the individual
           changes. The avatar uses the directory composable so it
           resolves through the same sync-engine pool as the rest of
           the app; actor uuid is null for system events (background
           jobs, webhooks), which fall back to the "sys" badge. -->
      <template v-for="item in timeline" :key="item.key">
        <!-- Grouped run of consecutive same-actor changes. -->
        <li v-if="item.kind === 'bundle'" class="flex flex-col">
          <button
            type="button"
            class="flex items-start gap-2 px-2 py-1.5 rounded hover:bg-surface-hover/40 transition-colors text-left w-full"
            :aria-expanded="expanded.has(item.anchorId)"
            @click="toggleBundle(item.anchorId)"
          >
            <UserAvatar
              v-if="actorFor(item.events[0]).kind === 'user' && actorFor(item.events[0]).userUuid"
              :uuid="actorFor(item.events[0]).userUuid!"
              size="xs"
              :show-name="false"
              :clickable="false"
              class="mt-0.5"
            />
            <span
              v-else
              class="inline-flex items-center justify-center w-5 h-5 rounded-full bg-surface-alt text-tertiary text-[9px] mt-0.5 shrink-0"
              :title="actorFor(item.events[0]).title ?? undefined"
            >sys</span>

            <div class="flex-1 min-w-0 text-xs text-secondary leading-relaxed">
              <span
                class="font-medium text-primary"
                :title="actorFor(item.events[0]).title ?? undefined"
              >{{ actorFor(item.events[0]).name }}</span>
              {{ t('ticket-activity-made-changes', { count: item.events.length }) }}
              <span class="text-tertiary tabular-nums">
                · {{ formatCompactRelativeTime(item.events[0].occurred_at) }}
              </span>
            </div>
            <Icon
              :name="expanded.has(item.anchorId) ? 'chevronUp' : 'chevronDown'"
              class="w-3.5 h-3.5 text-tertiary mt-1 shrink-0"
            />
          </button>

          <ul
            v-if="expanded.has(item.anchorId)"
            class="ml-7 mt-1 flex flex-col gap-1 border-l border-subtle pl-3"
          >
            <li
              v-for="ev in item.events"
              :key="ev.sync_id"
              class="text-xs text-secondary leading-relaxed"
            >
              {{ phraseFor(ev, ctx) }}
              <span v-if="assigneeNameFor(ev)" class="font-medium text-primary">
                {{ t('ticket-activity-to-assignee', { name: assigneeNameFor(ev) ?? '' }) }}
              </span>
              <span class="text-tertiary tabular-nums">
                · {{ formatCompactRelativeTime(ev.occurred_at) }}
              </span>
            </li>
          </ul>
        </li>

        <!-- Standalone event. `actorFor` picks the most meaningful
             identity: ticket sender for channel/portal events,
             signed-in user otherwise, "sys" badge as final fallback. -->
        <li
          v-else
          class="flex items-start gap-2 px-2 py-1.5 rounded transition-colors"
          :class="
            isInternalNoteEvent(item.events[0])
              ? 'bg-status-warning-bg/25 border-l-2 border-status-warning-border/60'
              : 'hover:bg-surface-hover/40'
          "
        >
          <template v-if="actorFor(item.events[0]).kind === 'user' && actorFor(item.events[0]).userUuid">
            <UserAvatar
              :uuid="actorFor(item.events[0]).userUuid!"
              size="xs"
              :show-name="false"
              :clickable="true"
              class="mt-0.5"
            />
          </template>
          <span
            v-else-if="actorFor(item.events[0]).kind === 'email'"
            class="inline-flex items-center justify-center w-5 h-5 rounded-full bg-status-info-bg text-status-info-border text-[10px] mt-0.5 shrink-0"
            :title="actorFor(item.events[0]).title ?? undefined"
            :aria-label="t('ticket-activity-actor-email-aria')"
          >@</span>
          <span
            v-else-if="actorFor(item.events[0]).kind === 'portal'"
            class="inline-flex items-center justify-center w-5 h-5 rounded-full bg-surface-alt text-tertiary text-[9px] mt-0.5 shrink-0"
            :title="actorFor(item.events[0]).title ?? undefined"
            :aria-label="t('ticket-activity-actor-portal-aria')"
          >www</span>
          <span
            v-else
            class="inline-flex items-center justify-center w-5 h-5 rounded-full bg-surface-alt text-tertiary text-[9px] mt-0.5 shrink-0"
            :title="actorFor(item.events[0]).title ?? undefined"
          >sys</span>

          <div class="flex-1 min-w-0 text-xs text-secondary leading-relaxed">
            <span
              class="font-medium text-primary"
              :title="actorFor(item.events[0]).title ?? undefined"
            >{{ actorFor(item.events[0]).name }}</span>
            {{ phraseFor(item.events[0], ctx) }}
            <span v-if="assigneeNameFor(item.events[0])" class="font-medium text-primary">
              {{ t('ticket-activity-to-assignee', { name: assigneeNameFor(item.events[0]) ?? '' }) }}
            </span>
            <span class="text-tertiary tabular-nums">
              · {{ formatCompactRelativeTime(item.events[0].occurred_at) }}
            </span>
          </div>
        </li>
      </template>
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
      <span>{{ loadingMore ? t('ticket-activity-loading') : t('ticket-activity-load-more') }}</span>
    </button>
  </div>
</template>
