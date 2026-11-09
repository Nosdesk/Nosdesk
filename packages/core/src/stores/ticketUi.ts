/**
 * Per-ticket ephemeral UI scratch state.
 *
 * In-memory only (no localStorage), keyed by ticket id. Holds the
 * bits that should survive nav-away/nav-back within a tab session
 * but don't deserve disk persistence: the files staged on the reply
 * composer (`File` objects can't be serialised; the user just picked
 * them, refreshing to a clean slate is fine).
 *
 * Each staged file uploads as soon as it is staged, on its own request
 * (see `attachmentUploadService`), and the upload lives here rather than
 * in the composer, so it carries on while the ticket is out of view. A
 * reply is sent with the uploaded ids. A file removed after it uploaded is
 * simply dropped: the server deletes unclaimed uploads after a day.
 *
 * Cleanup contract: callers that finish working with a ticket
 * (ticket deleted) call `clearAttachments`, which also stops its uploads.
 * Without explicit clears the map grows until the tab closes; that's
 * acceptable here (small, bounded by the number of tickets a user touches
 * in a session).
 *
 * Plugin-action activation counters used to live here too; they moved
 * to the frontend `usePluginActions` composable, which owns the plugin
 * concern end to end (core stays plugin-agnostic).
 */
import { defineStore } from 'pinia'
import { markRaw, reactive } from 'vue'
import {
  uploadAttachment,
  uploadFailure,
  type UploadedAttachment,
  type UploadFailure,
} from '../services/attachmentUploadService'
import { getTransportUploadCap } from '../services/uploadLimits'

export type StagedStatus = 'queued' | 'uploading' | 'uploaded' | 'failed'

/** A file on the reply composer and its upload. */
export interface StagedAttachment {
  /** Stable for the file's time on the composer, for list keys and actions. */
  key: string
  file: File
  /** An object URL of the file, for its preview. Revoked when it is removed. */
  previewUrl: string
  status: StagedStatus
  /** Bytes sent so far. */
  loaded: number
  /** The request's size, or null while the transport reports no progress. */
  total: number | null
  /** The stored file, once uploaded. */
  uploaded: UploadedAttachment | null
  /** Why the last attempt failed. */
  failure: UploadFailure | null
}

/** A voice note carries its transcription on the File. */
type FileWithTranscription = File & { _transcription?: string }

/**
 * How many files upload at once. The mobile app's bridge copies each request
 * body through memory, so there they go one at a time; elsewhere two, which
 * keeps the server's spool from filling with one person's many partial files.
 */
function uploadConcurrency(): number {
  return getTransportUploadCap() != null ? 1 : 2
}

let nextKey = 0

export const useTicketUiStore = defineStore('ticketUi', () => {
  const attachments = reactive(new Map<number, StagedAttachment[]>())
  /** Uploads waiting for a free slot, in the order they were staged. */
  const queue: StagedAttachment[] = []
  /** Running uploads by key, so a removal can stop one. */
  const controllers = new Map<string, AbortController>()
  let running = 0

  function getAttachments(ticketId: number): StagedAttachment[] {
    return attachments.get(ticketId) ?? []
  }

  function setList(ticketId: number, list: StagedAttachment[]): void {
    if (list.length === 0) attachments.delete(ticketId)
    else attachments.set(ticketId, list)
  }

  function pump(): void {
    while (running < uploadConcurrency() && queue.length > 0) {
      void run(queue.shift()!)
    }
  }

  async function run(entry: StagedAttachment): Promise<void> {
    const controller = new AbortController()
    controllers.set(entry.key, controller)
    running++
    entry.status = 'uploading'
    entry.loaded = 0
    entry.total = null
    entry.failure = null
    try {
      const stored = await uploadAttachment(entry.file, {
        transcription: (entry.file as FileWithTranscription)._transcription,
        signal: controller.signal,
        onProgress: (loaded, total) => {
          entry.loaded = loaded
          entry.total = total
        },
      })
      if (controller.signal.aborted) return
      entry.uploaded = stored
      entry.status = 'uploaded'
    } catch (err) {
      if (controller.signal.aborted) return
      entry.failure = uploadFailure(err) ?? 'failed'
      entry.status = 'failed'
    } finally {
      if (controllers.get(entry.key) === controller) controllers.delete(entry.key)
      running--
      pump()
    }
  }

  function enqueue(entry: StagedAttachment): void {
    entry.status = 'queued'
    entry.failure = null
    queue.push(entry)
  }

  /** Stop an upload, queued or running. */
  function stop(entry: StagedAttachment): void {
    const queued = queue.indexOf(entry)
    if (queued >= 0) queue.splice(queued, 1)
    controllers.get(entry.key)?.abort()
    controllers.delete(entry.key)
  }

  /** Stage files on a ticket's reply and start uploading them. */
  function stageFiles(ticketId: number, files: File[]): StagedAttachment[] {
    if (files.length === 0) return []
    const added = files.map((file) =>
      reactive<StagedAttachment>({
        key: `staged-${++nextKey}`,
        file: markRaw(file),
        previewUrl: URL.createObjectURL(file),
        status: 'queued',
        loaded: 0,
        total: null,
        uploaded: null,
        failure: null,
      }),
    )
    setList(ticketId, [...getAttachments(ticketId), ...added])
    added.forEach(enqueue)
    pump()
    return added
  }

  /** Upload a failed file again. */
  function retryAttachment(ticketId: number, key: string): void {
    const entry = getAttachments(ticketId).find((a) => a.key === key)
    if (!entry || entry.status !== 'failed') return
    enqueue(entry)
    pump()
  }

  /**
   * Take a file off the reply: its upload stops if it is still going, and an
   * uploaded one is left for the server to delete.
   */
  function removeAttachment(ticketId: number, key: string): void {
    const list = getAttachments(ticketId)
    const entry = list.find((a) => a.key === key)
    if (!entry) return
    stop(entry)
    URL.revokeObjectURL(entry.previewUrl)
    setList(
      ticketId,
      list.filter((a) => a !== entry),
    )
  }

  /**
   * Take every file off the reply as it is sent. Their previews stay valid:
   * the sent reply shows them, and a reply that fails comes back with them
   * (see `restoreAttachments`).
   */
  function takeAttachments(ticketId: number): StagedAttachment[] {
    const list = getAttachments(ticketId)
    attachments.delete(ticketId)
    return list
  }

  /** Put a failed reply's files back, before any staged since. */
  function restoreAttachments(ticketId: number, restored: StagedAttachment[]): void {
    setList(ticketId, [...restored, ...getAttachments(ticketId)])
  }

  /** Drop a ticket's staged files, stopping their uploads. */
  function clearAttachments(ticketId: number): void {
    const list = attachments.get(ticketId)
    if (!list) return
    for (const entry of list) {
      stop(entry)
      URL.revokeObjectURL(entry.previewUrl)
    }
    attachments.delete(ticketId)
  }

  return {
    getAttachments,
    stageFiles,
    retryAttachment,
    removeAttachment,
    takeAttachments,
    restoreAttachments,
    clearAttachments,
  }
})
