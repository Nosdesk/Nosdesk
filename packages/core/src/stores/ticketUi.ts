/**
 * Per-ticket ephemeral UI scratch state.
 *
 * In-memory only (no localStorage), keyed by ticket id. Holds the
 * bits that should survive nav-away/nav-back within a tab session
 * but don't deserve disk persistence: pending file attachments
 * selected for the comment composer (`File` objects can't be
 * serialised; the user just picked them, refreshing to a clean
 * slate is fine).
 *
 * Cleanup contract: callers that finish working with a ticket
 * (comment submitted, ticket deleted) call `clearAttachments` to
 * free memory. Without explicit clears the map grows until the tab
 * closes; that's acceptable here (small, bounded by the number of
 * tickets a user touches in a session).
 *
 * Plugin-action activation counters used to live here too; they moved
 * to the frontend `usePluginActions` composable, which owns the plugin
 * concern end to end (core stays plugin-agnostic).
 */
import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useTicketUiStore = defineStore('ticketUi', () => {
  const attachments = ref<Map<number, File[]>>(new Map())

  function getAttachments(ticketId: number): File[] {
    return attachments.value.get(ticketId) ?? []
  }

  function setAttachments(ticketId: number, files: File[]): void {
    const next = new Map(attachments.value)
    if (files.length === 0) {
      next.delete(ticketId)
    } else {
      next.set(ticketId, files)
    }
    attachments.value = next
  }

  function clearAttachments(ticketId: number): void {
    if (!attachments.value.has(ticketId)) return
    const next = new Map(attachments.value)
    next.delete(ticketId)
    attachments.value = next
  }

  return {
    getAttachments,
    setAttachments,
    clearAttachments,
  }
})
