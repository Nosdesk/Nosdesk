import apiClient from './apiConfig'

export type OsFamily = 'windows' | 'macos' | 'linux' | 'ios' | 'android' | 'other'
export type WarrantyBucket =
  | 'expired'
  | 'expiring_30d'
  | 'expiring_90d'
  | 'active'
  | 'unknown'

export interface AssetPlannerRow {
  id: number
  name: string
  hostname: string | null
  manufacturer: string | null
  model: string | null
  operating_system: string | null
  os_version: string | null
  os_family: OsFamily
  warranty_end_date: string | null
  warranty_bucket: WarrantyBucket
  compliance_state: string | null
  primary_user_uuid: string | null
  asset_tag: string | null
}

export const assetsService = {
  /** Returns every device shaped for the asset rollout planner.
   * The os_family and warranty_bucket fields are bucketed
   * server-side so the renderer can group / filter without
   * repeating heuristics. */
  async planner(): Promise<AssetPlannerRow[]> {
    const { data } = await apiClient.get<AssetPlannerRow[]>('/assets/planner')
    return data
  },
}
