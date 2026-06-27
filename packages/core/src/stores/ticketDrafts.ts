/**
 * Per-ticket draft store.
 *
 * Holds the in-progress comment text + internal/public flag for
 * each ticket id. Persisted to `localStorage` so drafts survive
 * navigation, hard refresh, and tab close — same UX guarantee
 * Linear and Notion give for in-progress comments.
 *
 * Lives outside `TicketView`'s component lifetime so dropping
 * `<KeepAlive>` doesn't lose unsaved text. Keyed by ticket id;
 * ids without a draft return the default empty draft (no
 * persistent entry is created until the user actually types).
 *
 * Attachments live in the sibling `useTicketUiStore` because
 * `File` objects can't be JSON-serialised — they survive
 * navigation but not refresh, which is the right trade-off
 * (the user picked them seconds ago, not days ago).
 */
import { defineStore } from 'pinia'
import { ref, watch } from 'vue'

import { logger } from '../utils/logger'
import { storage } from '../storage'

export interface TicketDraft {
  /** HTML content from the rich-text composer. */
  content: string
  /** Internal note flag (tech-to-tech vs public reply). */
  isInternal: boolean
}

const STORAGE_KEY = 'nosdesk:ticket-drafts'
const PERSIST_DEBOUNCE_MS = 400

const EMPTY_DRAFT: TicketDraft = Object.freeze({
  content: '',
  isInternal: false,
})

function loadFromStorage(): Map<number, TicketDraft> {
  try {
    const raw = storage().getItem(STORAGE_KEY)
    if (!raw) return new Map()
    const parsed = JSON.parse(raw) as Record<string, TicketDraft>
    const out = new Map<number, TicketDraft>()
    for (const [k, v] of Object.entries(parsed)) {
      const id = Number(k)
      if (Number.isFinite(id) && v && typeof v.content === 'string') {
        out.set(id, { content: v.content, isInternal: !!v.isInternal })
      }
    }
    return out
  } catch (err) {
    logger.warn('Failed to load ticket drafts from localStorage', { err })
    return new Map()
  }
}

function persistToStorage(drafts: Map<number, TicketDraft>): void {
  try {
    if (drafts.size === 0) {
      storage().removeItem(STORAGE_KEY)
      return
    }
    const obj: Record<string, TicketDraft> = {}
    for (const [id, draft] of drafts) obj[String(id)] = draft
    storage().setItem(STORAGE_KEY, JSON.stringify(obj))
  } catch (err) {
    // QuotaExceededError, JSON failure, or sandboxed storage.
    // Drafts still work in memory; just no persistence.
    logger.warn('Failed to persist ticket drafts', { err })
  }
}

export const useTicketDraftsStore = defineStore('ticketDrafts', () => {
  const drafts = ref<Map<number, TicketDraft>>(loadFromStorage())

  // Debounced persist on any mutation.
  let persistHandle: ReturnType<typeof setTimeout> | null = null
  watch(
    drafts,
    () => {
      if (persistHandle) clearTimeout(persistHandle)
      persistHandle = setTimeout(() => {
        persistToStorage(drafts.value)
        persistHandle = null
      }, PERSIST_DEBOUNCE_MS)
    },
    { deep: true },
  )

  /** Returns the current draft for `ticketId`, or the empty
   *  draft when no entry exists. The returned object is frozen
   *  for the empty case so callers can't mutate the shared
   *  default. Callers wanting to write should call `setDraft`. */
  function getDraft(ticketId: number): TicketDraft {
    return drafts.value.get(ticketId) ?? EMPTY_DRAFT
  }

  /** Replace (or remove if empty) the draft for `ticketId`. */
  function setDraft(ticketId: number, draft: TicketDraft): void {
    const next = new Map(drafts.value)
    if (!draft.content && !draft.isInternal) {
      next.delete(ticketId)
    } else {
      next.set(ticketId, { content: draft.content, isInternal: !!draft.isInternal })
    }
    drafts.value = next
  }

  /** Drop the draft for `ticketId`, e.g. after a successful
   *  comment submission. */
  function clearDraft(ticketId: number): void {
    if (!drafts.value.has(ticketId)) return
    const next = new Map(drafts.value)
    next.delete(ticketId)
    drafts.value = next
  }

  return {
    getDraft,
    setDraft,
    clearDraft,
  }
})
