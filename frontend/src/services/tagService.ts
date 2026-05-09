/**
 * Tag REST client. Pairs with the workspace tag store
 * (`stores/tags.ts`) which caches the list across the session;
 * direct callers exist for the admin tag-management UI.
 */
import apiClient from './apiConfig'
import type { Tag, NewTagPayload, TagUpdatePayload } from '@/types/tag'

export const tagService = {
  async list(includeArchived = false): Promise<Tag[]> {
    const response = await apiClient.get<Tag[]>('/tags', {
      params: { include_archived: includeArchived },
    })
    return response.data ?? []
  },

  async create(body: NewTagPayload): Promise<Tag> {
    const response = await apiClient.post<Tag>('/tags', body)
    return response.data
  },

  async update(id: number, body: TagUpdatePayload): Promise<Tag> {
    const response = await apiClient.patch<Tag>(`/tags/${id}`, body)
    return response.data
  },

  async archive(id: number): Promise<Tag> {
    const response = await apiClient.delete<Tag>(`/tags/${id}`)
    return response.data
  },

  /** Replace the tag set on a ticket. Empty array clears all
   *  tags. Returns the resulting tag id list. */
  async setForTicket(ticketId: number, tagIds: number[]): Promise<number[]> {
    const response = await apiClient.put<{ tag_ids: number[] }>(
      `/tickets/${ticketId}/tags`,
      { tag_ids: tagIds },
    )
    return response.data.tag_ids
  },
}
