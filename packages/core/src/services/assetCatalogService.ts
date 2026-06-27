import apiClient from '../apiClient'
import type { Manufacturer, AssetModel } from '../types/asset'

/** Shared Pinia Colada cache keys for the make/model catalog. Inline
 *  quick-create invalidates these so every open picker refreshes. */
export const MANUFACTURERS_QUERY_KEY = ['manufacturers'] as const
export const ASSET_MODELS_QUERY_KEY = ['asset-models'] as const

export interface CreateManufacturerBody {
  name: string
}

export interface CreateAssetModelBody {
  manufacturer_id: number
  name: string
  kind: string
  part_number?: string | null
  default_attributes?: Record<string, unknown>
  notes?: string | null
}

export interface UpdateAssetModelBody {
  manufacturer_id?: number
  name?: string
  kind?: string
  /** `null` clears, omit to leave unchanged. */
  part_number?: string | null
  default_attributes?: Record<string, unknown>
  notes?: string | null
}

export const manufacturersService = {
  async list(): Promise<Manufacturer[]> {
    const { data } = await apiClient.get<Manufacturer[]>('/manufacturers')
    return data
  },
  async create(body: CreateManufacturerBody): Promise<Manufacturer> {
    const { data } = await apiClient.post<Manufacturer>('/manufacturers', body)
    return data
  },
  async update(id: number, body: CreateManufacturerBody): Promise<Manufacturer> {
    const { data } = await apiClient.put<Manufacturer>(`/manufacturers/${id}`, body)
    return data
  },
  async delete(id: number): Promise<void> {
    await apiClient.delete(`/manufacturers/${id}`)
  },
}

export const assetModelsService = {
  async list(): Promise<AssetModel[]> {
    const { data } = await apiClient.get<AssetModel[]>('/asset-models')
    return data
  },
  async create(body: CreateAssetModelBody): Promise<AssetModel> {
    const { data } = await apiClient.post<AssetModel>('/asset-models', body)
    return data
  },
  async update(id: number, body: UpdateAssetModelBody): Promise<AssetModel> {
    const { data } = await apiClient.put<AssetModel>(`/asset-models/${id}`, body)
    return data
  },
  async delete(id: number): Promise<void> {
    await apiClient.delete(`/asset-models/${id}`)
  },
}
