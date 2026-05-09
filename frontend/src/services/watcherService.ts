/**
 * Ticket watcher REST client. Watch / unwatch operate on the
 * authenticated user — no body, the JWT identifies the watcher.
 */
import apiClient from './apiConfig'

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
}
