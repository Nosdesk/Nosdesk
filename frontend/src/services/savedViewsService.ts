import apiClient from './apiConfig'
import type { ViewShape, FilterState } from '@/sync/views/types'

export interface SavedView {
  id: number
  uuid: string
  scope: 'workspace' | 'project' | 'private'
  scope_id: string | null
  name: string
  shape: ViewShape
  filter: FilterState
  created_by: string
  created_at: string
  updated_at: string
}

export interface CreateSavedViewBody {
  scope: SavedView['scope']
  scope_id?: string | null
  name: string
  shape: ViewShape
  filter: FilterState
}

export interface UpdateSavedViewBody {
  name?: string
  shape?: ViewShape
  filter?: FilterState
}

export const savedViewsService = {
  async list(projectId?: number): Promise<SavedView[]> {
    const params = projectId == null ? {} : { project_id: projectId }
    const { data } = await apiClient.get<SavedView[]>('/saved-views', { params })
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

  async update(uuid: string, body: UpdateSavedViewBody): Promise<SavedView> {
    const { data } = await apiClient.patch<SavedView>(`/saved-views/${uuid}`, body)
    return data
  },

  async delete(uuid: string): Promise<void> {
    await apiClient.delete(`/saved-views/${uuid}`)
  },
}
