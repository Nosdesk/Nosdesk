/**
 * Saved-views API client.
 *
 * Generic over the `shape` (display config: columns, grouping,
 * density, ...) and `filter` (predicate / facet selections) JSONB
 * blobs so each list view can carry its own typed payload while
 * the wire shape stays uniform. Tickets default to the existing
 * `ViewShape` + `FilterState` types so their consumers keep
 * working without explicit type args. Asset and user views pass
 * their own types to the dataset helpers below.
 *
 * The backend's `dataset` column discriminates the row's surface
 * ('tickets' | 'assets' | 'users'); the handler refuses
 * workspace / project scope for non-ticket datasets so the
 * permission model stays ticket-specific.
 */
import apiClient from './apiConfig'
import type { ViewShape, FilterState } from '@/sync/views/types'

export type SavedViewDataset = 'tickets' | 'assets' | 'users'
export type SavedViewScope = 'workspace' | 'project' | 'private'

export interface SavedView<S = ViewShape, F = FilterState> {
  id: number
  uuid: string
  scope: SavedViewScope
  scope_id: string | null
  name: string
  shape: S
  filter: F
  created_by: string
  created_at: string
  updated_at: string
  /** Optional in the wire response because older clients never
   *  read it; new code can assume the backend always sends it
   *  post-migration. */
  dataset?: SavedViewDataset
}

export interface CreateSavedViewBody<S = ViewShape, F = FilterState> {
  scope: SavedViewScope
  scope_id?: string | null
  name: string
  shape: S
  filter: F
  /** Defaults to 'tickets' on the backend when omitted. Non-
   *  ticket datasets must always set this AND scope = 'private'. */
  dataset?: SavedViewDataset
}

export interface UpdateSavedViewBody<S = ViewShape, F = FilterState> {
  name?: string
  shape?: S
  filter?: F
}

export const savedViewsService = {
  async list(projectId?: number): Promise<SavedView[]> {
    const params = projectId == null ? {} : { project_id: projectId }
    const { data } = await apiClient.get<SavedView[]>('/saved-views', { params })
    return data
  },

  /** Per-dataset listing for asset / user surfaces. Backend
   *  returns only the caller's private views for the dataset; no
   *  workspace / project scope merging because those scopes are
   *  ticket-specific. */
  async listForDataset<S = unknown, F = unknown>(
    dataset: Exclude<SavedViewDataset, 'tickets'>,
  ): Promise<SavedView<S, F>[]> {
    const { data } = await apiClient.get<SavedView<S, F>[]>('/saved-views', {
      params: { dataset },
    })
    return data
  },

  async get(uuid: string): Promise<SavedView> {
    const { data } = await apiClient.get<SavedView>(`/saved-views/${uuid}`)
    return data
  },

  async create(body: CreateSavedViewBody): Promise<SavedView> {
    const { data } = await apiClient.post<SavedView>('/saved-views', body)
    return data
  },

  async createForDataset<S, F>(
    body: CreateSavedViewBody<S, F>,
  ): Promise<SavedView<S, F>> {
    const { data } = await apiClient.post<SavedView<S, F>>('/saved-views', body)
    return data
  },

  async update(uuid: string, body: UpdateSavedViewBody): Promise<SavedView> {
    const { data } = await apiClient.patch<SavedView>(
      `/saved-views/${uuid}`,
      body,
    )
    return data
  },

  async updateForDataset<S, F>(
    uuid: string,
    body: UpdateSavedViewBody<S, F>,
  ): Promise<SavedView<S, F>> {
    const { data } = await apiClient.patch<SavedView<S, F>>(
      `/saved-views/${uuid}`,
      body,
    )
    return data
  },

  async delete(uuid: string): Promise<void> {
    await apiClient.delete(`/saved-views/${uuid}`)
  },
}
