// The embeddable help widget's settings.
import apiClient from '../apiClient'

export interface WidgetSettings {
  enabled: boolean
  /** Sites allowed to show the widget (`https://help.acme.com`). */
  allowed_origins: string[]
  /** Visitors without a signed identity get help articles and a request form. */
  allow_anonymous: boolean
  /** A signing secret has been generated. */
  has_secret: boolean
  /** What the site's script tag points at. */
  script_url: string | null
}

export const widgetService = {
  async get(): Promise<WidgetSettings> {
    const { data } = await apiClient.get<WidgetSettings>('/admin/widget')
    return data
  },
  async save(input: {
    enabled: boolean
    allowed_origins: string[]
    allow_anonymous: boolean
  }): Promise<WidgetSettings> {
    const { data } = await apiClient.put<WidgetSettings>('/admin/widget', input)
    return data
  },
  /** A new signing secret, returned once. */
  async rotateSecret(): Promise<string> {
    const { data } = await apiClient.post<{ secret: string }>('/admin/widget/secret')
    return data.secret
  },
}
