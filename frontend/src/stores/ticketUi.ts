/**
 * Per-ticket ephemeral UI scratch state.
 *
 * In-memory only (no localStorage), keyed by ticket id. Holds the
 * bits that should survive nav-away/nav-back within a tab session
 * but don't deserve disk persistence:
 *
 *   - Pending file attachments selected for the comment composer
 *     (`File` objects can't be serialised; the user just picked
 *     them, refreshing to a clean slate is fine).
 *   - Plugin-action activation counters (signal a plugin sidebar
 *     panel to perform a domain action, rendered next-tick).
 *
 * Cleanup contract: callers that finish working with a ticket
 * (comment submitted, panel torn down) call the matching
 * `clear*` to free memory. Without explicit clears the maps
 * grow until the tab closes; that's acceptable for these data
 * shapes (small, bounded by the number of tickets a user touches
 * in a session).
 */
import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useTicketUiStore = defineStore('ticketUi', () => {
  const attachments = ref<Map<number, File[]>>(new Map())
  const pluginActivations = ref<Map<number, Map<string, number>>>(new Map())

  // ---- Attachments ------------------------------------------------

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

  // ---- Plugin activation map -------------------------------------
  // Each plugin sidebar item that's `activate`d gets its counter
  // bumped. The PluginSlot watches the counter and re-runs the
  // action's effect on each tick. Per-ticket so distinct tickets
  // don't share activation state.

  function getPluginActivations(ticketId: number): Map<string, number> {
    return pluginActivations.value.get(ticketId) ?? new Map()
  }

  function activatePluginAction(ticketId: number, key: string): void {
    const next = new Map(pluginActivations.value)
    const existing = next.get(ticketId) ?? new Map<string, number>()
    const updated = new Map(existing)
    updated.set(key, (updated.get(key) ?? 0) + 1)
    next.set(ticketId, updated)
    pluginActivations.value = next
  }

  function clearPluginActivations(ticketId: number): void {
    if (!pluginActivations.value.has(ticketId)) return
    const next = new Map(pluginActivations.value)
    next.delete(ticketId)
    pluginActivations.value = next
  }

  return {
    // Attachments
    getAttachments,
    setAttachments,
    clearAttachments,
    // Plugin activations
    getPluginActivations,
    activatePluginAction,
    clearPluginActivations,
  }
})
