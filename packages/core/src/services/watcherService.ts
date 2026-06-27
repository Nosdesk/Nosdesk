/**
 * Ticket watcher REST client. Watch / unwatch operate on the
 * authenticated user; the JWT identifies the watcher so no body
 * is required for those calls.
 */
import apiClient from '../apiClient'

export interface MyWatchState {
  watching: boolean
  notify_on_internal_notes: boolean
  auto_added: boolean
}

export const watcherService = {
  async list(ticketId: number): Promise<string[]> {
    const response = await apiClient.get<{ watcher_uuids: string[] }>(
      `/tickets/${ticketId}/watchers`,
    )
    return response.data.watcher_uuids ?? []
  },

  async watch(ticketId: number): Promise<void> {
    await apiClient.post(`/tickets/${ticketId}/watch`)
  },

  async unwatch(ticketId: number): Promise<void> {
    await apiClient.delete(`/tickets/${ticketId}/watch`)
  },

  /** Per-user watch state on a ticket (including the
   *  notify-on-internal preference). Returns sensible defaults
   *  when the user isn't watching, so the UI can render the
   *  toggle stub without an extra null-check round-trip. */
  async myState(ticketId: number): Promise<MyWatchState> {
    const response = await apiClient.get<MyWatchState>(
      `/tickets/${ticketId}/watch/me`,
    )
    return response.data
  },

  /** Update the authenticated user's per-watch preferences. */
  async updatePreferences(
    ticketId: number,
    prefs: { notify_on_internal_notes: boolean },
  ): Promise<void> {
    await apiClient.patch(`/tickets/${ticketId}/watch/preferences`, prefs)
  },
}
