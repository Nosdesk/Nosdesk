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
import { htmlText } from '../utils/inertHtml'
import { storage } from '../storage'

export interface TicketDraft {
  /** HTML content from the rich-text composer. */
  content: string
  /** Internal note flag (tech-to-tech vs public reply). */
  isInternal: boolean
  /** Set on a reply put back after it failed to send. See `resendClientId`. */
  resend?: ResendMark
}

/** A reply put back in the composer after it failed to send, as it was. */
export interface ResendMark {
  /** The id it went out with. */
  clientId: string
  /** Its text, without markup. */
  text: string
  /** Its files, as `name:size`. */
  files: string[]
}

function fileMark(file: File): string {
  return `${file.name}:${file.size}`
}

/** What a failed reply's draft records so it can be recognised unchanged. */
export function resendMark(clientId: string, content: string, files: File[]): ResendMark {
  return { clientId, text: htmlText(content), files: files.map(fileMark) }
}

/**
 * The id to send a draft with: the one its reply first went out with while
 * it is still that reply (the same text and files), so a server that already
 * has it can tell; none once it has been edited or gained or lost a file,
 * which makes it a new reply. The text is compared without markup, since the
 * editor may write the same text back differently.
 */
export function resendClientId(draft: TicketDraft, files: File[]): string | undefined {
  const mark = draft.resend
  if (!mark) return undefined
  const sameFiles =
    files.length === mark.files.length && files.every((f, i) => fileMark(f) === mark.files[i])
  return sameFiles && htmlText(draft.content) === mark.text ? mark.clientId : undefined
}

function isResendMark(v: unknown): v is ResendMark {
  const m = v as ResendMark | undefined
  return (
    !!m &&
    typeof m.clientId === 'string' &&
    typeof m.text === 'string' &&
    Array.isArray(m.files) &&
    m.files.every((f) => typeof f === 'string')
  )
}

const STORAGE_KEY = 'nosdesk:ticket-drafts'
/** Ticket ids repeat across workspaces on a single-origin instance, so
 *  each workspace's drafts live under their own key. `null` (host mode,
 *  or before a workspace is selected) keeps the original key. */
let scope: string | null = null
function storageKey(): string {
  return scope ? `${STORAGE_KEY}:${scope}` : STORAGE_KEY
}
const PERSIST_DEBOUNCE_MS = 400

const EMPTY_DRAFT: TicketDraft = Object.freeze({
  content: '',
  isInternal: false,
})

function loadFromStorage(): Map<number, TicketDraft> {
  try {
    const raw = storage().getItem(storageKey())
    if (!raw) return new Map()
    const parsed = JSON.parse(raw) as Record<string, TicketDraft>
    const out = new Map<number, TicketDraft>()
    for (const [k, v] of Object.entries(parsed)) {
      const id = Number(k)
      if (Number.isFinite(id) && v && typeof v.content === 'string') {
        out.set(id, {
          content: v.content,
          isInternal: !!v.isInternal,
          ...(isResendMark(v.resend) ? { resend: v.resend } : {}),
        })
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
      storage().removeItem(storageKey())
      return
    }
    const obj: Record<string, TicketDraft> = {}
    for (const [id, draft] of drafts) obj[String(id)] = draft
    storage().setItem(storageKey(), JSON.stringify(obj))
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
      next.set(ticketId, {
        content: draft.content,
        isInternal: !!draft.isInternal,
        ...(draft.resend ? { resend: draft.resend } : {}),
      })
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

  /** Switch to `workspaceSlug`'s drafts: flush the current set to its
   *  own key, then load the other workspace's. Called by the workspace
   *  reset, so a switch never shows one workspace's draft on another's
   *  ticket of the same id. */
  function setScope(workspaceSlug: string | null): void {
    if (workspaceSlug === scope) return
    if (persistHandle) {
      clearTimeout(persistHandle)
      persistHandle = null
    }
    persistToStorage(drafts.value)
    scope = workspaceSlug
    drafts.value = loadFromStorage()
  }

  /** The workspace whose drafts are loaded; `null` in host mode, or while
   *  a workspace switch is under way. */
  function getScope(): string | null {
    return scope
  }

  return {
    getDraft,
    setDraft,
    clearDraft,
    setScope,
    getScope,
  }
})
