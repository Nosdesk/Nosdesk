import { computed, onScopeDispose, ref, shallowRef, watch, type Ref } from 'vue'
import type { Awareness } from 'y-protocols/awareness'
import { useAuthStore } from '@/stores/auth'
import { useCollabSessionStore } from '@/stores/collabSession'
import { useSyncTicketsStore } from '@/sync/stores/tickets'
import { useCollabDocId } from '@/composables/useCollabDocId'
import type { ViewerInfo } from '@nosdesk/core/types/sse'

/** A field a ticket viewer can be seen typing into. */
export type PreviewField = 'title'

/** What a client publishes in the room's awareness while typing. */
interface PreviewState {
  field: PreviewField
  value: string
}

/** How often a client republishes what it is typing. */
const PREVIEW_INTERVAL_MS = 150
/** How long a colleague's last preview stays shown after they stop, waiting
 *  for their saved value to arrive through sync. */
const PREVIEW_LINGER_MS = 3000
/** The longest preview a client publishes, matching the title's limit. */
const PREVIEW_MAX_CHARS = 500

/**
 * Who else is on a ticket, and what they are typing, from the awareness of
 * the ticket's collaboration room.
 *
 * Every ticket view holds that room open for the ticket's notes, and the
 * server routes everyone on a ticket to the one machine that owns its room,
 * so this works however many machines serve the app. The server attests
 * each awareness entry (`services/yjs/awareness.rs`): a client speaks only
 * for its own Yjs client, and `user.uuid` is its signed-in user.
 *
 * The room belongs to the notes editor; this only reads its awareness and
 * publishes a `preview` field in it. Ephemeral state stays out of the sync
 * pool: a colleague's typing is shown over the ticket's title, never written
 * into its row.
 */
export function useTicketPresence(ticketId: Ref<number | undefined>) {
  const auth = useAuthStore()
  const collab = useCollabSessionStore()
  const tickets = useSyncTicketsStore()
  const docId = useCollabDocId('ticket', () =>
    ticketId.value == null ? null : (tickets.byId(ticketId.value).value?.uuid ?? null),
  )

  // What this client is about to publish (`undefined`: nothing pending).
  let pending: PreviewState | null | undefined
  let publishTimer: ReturnType<typeof setTimeout> | null = null

  const awareness = shallowRef<Awareness | null>(null)
  // Bumped on every awareness change, so the computeds below re-read it.
  const revision = ref(0)
  const bump = () => {
    revision.value++
  }

  watch(
    () => {
      // Re-resolve when sessions open or close, since the editor's room can be
      // evicted and reopened under the same id.
      void collab.sessionSnapshot
      return docId.value ? (collab.peek(docId.value)?.provider.awareness ?? null) : null
    },
    (next, previous) => {
      if (next === previous) return
      previous?.off('change', bump)
      clearOwnPreview(previous)
      awareness.value = next
      next?.on('change', bump)
      bump()
    },
    { immediate: true },
  )

  /** Everyone else on the ticket, once per person, most recently active first. */
  const otherViewers = computed<ViewerInfo[]>(() => {
    void revision.value
    const room = awareness.value
    const self = auth.user?.uuid
    if (!room) return []
    const latest = new Map<string, number>()
    room.getStates().forEach((state, clientId) => {
      if (clientId === room.clientID) return
      const uuid = (state as { user?: { uuid?: unknown } })?.user?.uuid
      if (typeof uuid !== 'string' || uuid === self) return
      const seen = room.meta.get(clientId)?.lastUpdated ?? 0
      latest.set(uuid, Math.max(latest.get(uuid) ?? 0, seen))
    })
    return [...latest.entries()]
      .sort((a, b) => b[1] - a[1])
      .map(([user_uuid, at]) => ({ user_uuid, last_active_at: new Date(at).toISOString() }))
  })

  /** The latest preview a colleague is publishing for each field. */
  const livePreviews = computed(() => {
    void revision.value
    const room = awareness.value
    const out = new Map<PreviewField, { value: string; at: number }>()
    if (!room) return out
    room.getStates().forEach((state, clientId) => {
      if (clientId === room.clientID) return
      const preview = (state as { preview?: PreviewState | null })?.preview
      if (!preview || typeof preview.value !== 'string') return
      const at = room.meta.get(clientId)?.lastUpdated ?? 0
      const current = out.get(preview.field)
      if (!current || at > current.at) out.set(preview.field, { value: preview.value, at })
    })
    return out
  })

  // A preview that stopped stays shown briefly, so the field doesn't flick
  // back to the old value before the saved one arrives through sync. It
  // covers the saved value only while that is still what it was when the
  // preview stopped.
  const savedOf = new Map<PreviewField, () => string | undefined>()
  const lingering = ref(new Map<PreviewField, { value: string; over: string | undefined }>())
  const lingerTimers = new Map<PreviewField, ReturnType<typeof setTimeout>>()
  const stopLingering = (field: PreviewField) => {
    lingering.value.delete(field)
    clearTimeout(lingerTimers.get(field))
    lingerTimers.delete(field)
  }
  watch(livePreviews, (now, before) => {
    for (const [field, { value }] of before ?? []) {
      if (now.has(field)) continue
      lingering.value.set(field, { value, over: savedOf.get(field)?.() })
      clearTimeout(lingerTimers.get(field))
      lingerTimers.set(
        field,
        setTimeout(() => stopLingering(field), PREVIEW_LINGER_MS),
      )
    }
    for (const field of now.keys()) stopLingering(field)
  })

  /**
   * A colleague's in-progress value for `field`, to show over `saved`, the
   * field's value in the pool; `null` when no one is typing in it.
   */
  function previewOver(field: PreviewField, saved: () => string | undefined) {
    savedOf.set(field, saved)
    return computed<string | null>(() => {
      const live = livePreviews.value.get(field)
      if (live) return live.value
      const last = lingering.value.get(field)
      return last && saved() === last.over ? last.value : null
    })
  }

  // Publishing. Throttled, so typing doesn't flood the room.
  function flush() {
    publishTimer = null
    if (pending === undefined) return
    awareness.value?.setLocalStateField('preview', pending)
    pending = undefined
  }
  /** Show colleagues what is being typed into `field`; `null` stops. */
  function publishPreview(field: PreviewField, value: string | null) {
    pending = value === null ? null : { field, value: value.slice(0, PREVIEW_MAX_CHARS) }
    if (value === null) {
      if (publishTimer) clearTimeout(publishTimer)
      flush()
      return
    }
    if (!publishTimer) publishTimer = setTimeout(flush, PREVIEW_INTERVAL_MS)
  }
  function clearOwnPreview(room: Awareness | null | undefined) {
    if (publishTimer) clearTimeout(publishTimer)
    publishTimer = null
    pending = undefined
    const own = room?.getLocalState() as { preview?: unknown } | null | undefined
    if (own?.preview) room?.setLocalStateField('preview', null)
  }

  onScopeDispose(() => {
    awareness.value?.off('change', bump)
    clearOwnPreview(awareness.value)
    for (const timer of lingerTimers.values()) clearTimeout(timer)
  })

  return { otherViewers, previewOver, publishPreview }
}
