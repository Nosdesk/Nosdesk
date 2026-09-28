// The Microsoft Teams personal tab's settings.
import apiClient from '../apiClient'

export interface TeamsSettings {
  /** Requester sign-in is set up with Microsoft Entra ID (the tab needs it). */
  available: boolean
  /** The requester sign-in provider's kind, if one is set up. */
  provider_kind: string | null
  enabled: boolean
  /** The workspace's Entra app (application/client id). */
  client_id: string | null
  allowed_domains: string[]
  package_version: string
  /** What the admin sets on their Entra app registration. */
  setup: {
    redirect_uri: string
    application_id_uri: string
    scope: string
    tab_url: string
  } | null
}

export const teamsService = {
  async get(): Promise<TeamsSettings> {
    const { data } = await apiClient.get<TeamsSettings>('/admin/teams')
    return data
  },
  async save(enabled: boolean): Promise<TeamsSettings> {
    const { data } = await apiClient.put<TeamsSettings>('/admin/teams', { enabled })
    return data
  },
  /** The app package (a zip) the Teams admin uploads. */
  async downloadPackage(): Promise<Blob> {
    const { data } = await apiClient.get<Blob>('/admin/teams/package', { responseType: 'blob' })
    return data
  },
}
