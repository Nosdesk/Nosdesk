/**
 * Per-ticket draft store.
 *
 * Holds the in-progress comment text + internal/public flag for
 * each ticket id. Persisted to `localStorage` so drafts survive
 * navigation, hard refresh, and tab close, the same guarantee
 * Linear and Notion give for in-progress comments. A draft belongs
 * to the account that wrote it, on its server and in its workspace,
 * and is removed when that account signs out.
 *
 * Lives outside `TicketView`'s component lifetime so dropping
 * `<KeepAlive>` doesn't lose unsaved text. Keyed by ticket id;
 * ids without a draft return the default empty draft (no
 * persistent entry is created until the user actually types).
 *
 * Attachments live in the sibling `useTicketUiStore` because
 * `File` objects can't be JSON-serialised: they survive
 * navigation but not refresh, which is the right trade-off
 * (the user picked them seconds ago, not days ago).
 */
import { defineStore } from 'pinia'
import { ref } from 'vue'

import { logger } from '../utils/logger'
import { htmlText } from '../utils/inertHtml'
import { storage } from '../storage'
import { apiBaseUrl } from '../transport'

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

/** One entry per draft, so a tab writes only the drafts it changed and never
 *  erases another tab's. The key carries the account, the server and the
 *  workspace (ticket ids repeat across workspaces and servers), so a draft
 *  shows only to the person who wrote it, where they wrote it. */
const KEY_PREFIX = 'nosdesk:ticket-draft:'
/** Drafts from before the key carried an account: one map per workspace, or
 *  one in host mode. Nothing records who wrote them, so they are dropped
 *  rather than shown to whoever signs in next. */
const LEGACY_KEY = 'nosdesk:ticket-drafts'

const EMPTY_DRAFT: TicketDraft = Object.freeze({
  content: '',
  isInternal: false,
})

function isLegacyKey(key: string): boolean {
  return key === LEGACY_KEY || key.startsWith(`${LEGACY_KEY}:`)
}

/** The server the app talks to: the page's origin on the web, the chosen
 *  server in the mobile app. Empty before the transport is configured. */
function serverOrigin(): string {
  try {
    return new URL(apiBaseUrl(), globalThis.location?.href).origin
  } catch {
    return ''
  }
}

function accountPrefix(account: string): string {
  return `${KEY_PREFIX}${encodeURIComponent(account)}:`
}

function scopePrefix(account: string, workspace: string | null): string {
  const server = encodeURIComponent(serverOrigin())
  return `${accountPrefix(account)}${server}:${encodeURIComponent(workspace ?? '')}:`
}

function removeKeys(match: (key: string) => boolean): void {
  try {
    const store = storage()
    for (const key of store.keys().filter(match)) store.removeItem(key)
  } catch (err) {
    logger.warn('Failed to remove ticket drafts', { err })
  }
}

/**
 * Remove every stored draft, whoever wrote it. Called at sign-out and when the
 * mobile app leaves its server: storage holds one signed-in account at a time
 * (one session per origin, one server in the app), so once it is gone no
 * stored draft belongs to anyone signed in.
 */
export function removeAllTicketDrafts(): void {
  removeKeys((key) => key.startsWith(KEY_PREFIX) || isLegacyKey(key))
}

function parseDraft(raw: string | null): TicketDraft | null {
  if (!raw) return null
  const v = JSON.parse(raw) as Partial<TicketDraft> | null
  if (!v || typeof v.content !== 'string') return null
  return {
    content: v.content,
    isInternal: !!v.isInternal,
    ...(isResendMark(v.resend) ? { resend: v.resend } : {}),
  }
}

function loadDrafts(prefix: string): Map<number, TicketDraft> {
  const out = new Map<number, TicketDraft>()
  try {
    const store = storage()
    for (const key of store.keys()) {
      if (!key.startsWith(prefix)) continue
      const id = key.slice(prefix.length)
      if (!/^\d+$/.test(id)) continue
      const draft = parseDraft(store.getItem(key))
      if (draft) out.set(Number(id), draft)
    }
  } catch (err) {
    logger.warn('Failed to load ticket drafts', { err })
  }
  return out
}

export const useTicketDraftsStore = defineStore('ticketDrafts', () => {
  const drafts = ref<Map<number, TicketDraft>>(new Map())
  /** The signed-in account; drafts are neither loaded nor stored without one. */
  let account: string | null = null
  /** The workspace whose drafts are loaded; `null` in host mode, or while a
   *  workspace switch is under way. */
  let workspace: string | null = null
  /** Where this account's drafts for this server and workspace are stored. */
  let prefix: string | null = null

  function reload(): void {
    prefix = account ? scopePrefix(account, workspace) : null
    drafts.value = prefix ? loadDrafts(prefix) : new Map()
  }

  function persist(ticketId: number, draft: TicketDraft | null): void {
    if (!prefix) return
    try {
      if (draft) storage().setItem(`${prefix}${ticketId}`, JSON.stringify(draft))
      else storage().removeItem(`${prefix}${ticketId}`)
    } catch (err) {
      // QuotaExceededError or sandboxed storage. Drafts still work in
      // memory; just no persistence.
      logger.warn('Failed to persist a ticket draft', { err })
    }
  }

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
      persist(ticketId, null)
    } else {
      const kept: TicketDraft = {
        content: draft.content,
        isInternal: !!draft.isInternal,
        ...(draft.resend ? { resend: draft.resend } : {}),
      }
      next.set(ticketId, kept)
      persist(ticketId, kept)
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
    persist(ticketId, null)
  }

  /** Switch to `workspaceSlug`'s drafts. Called by the workspace reset and
   *  `enterWorkspace`, so a switch never shows one workspace's draft on
   *  another's ticket of the same id. */
  function setScope(workspaceSlug: string | null): void {
    if (workspaceSlug === workspace) return
    workspace = workspaceSlug
    reload()
  }

  /** Switch to `uuid`'s drafts as the signed-in account changes. An account
   *  another signs in over has left this tab, so its stored drafts go with
   *  it. With no account nothing is loaded, and stored drafts stay for the
   *  account to sign back in to (a lost session); a sign-out removes them,
   *  see `signOut`. */
  function setAccount(uuid: string | null): void {
    if (uuid === account) return
    if (account !== null && uuid !== null) {
      const leaving = accountPrefix(account)
      removeKeys((key) => key.startsWith(leaving))
    }
    if (uuid !== null) removeKeys(isLegacyKey)
    account = uuid
    reload()
  }

  /** Sign-out: every stored draft goes, and none is held in memory. */
  function signOut(): void {
    removeAllTicketDrafts()
    account = null
    workspace = null
    reload()
  }

  /** Whose drafts are loaded, for which server and workspace; `null` while no
   *  account is signed in. */
  function getScope(): string | null {
    return prefix
  }

  return {
    getDraft,
    setDraft,
    clearDraft,
    setScope,
    setAccount,
    signOut,
    getScope,
  }
})
