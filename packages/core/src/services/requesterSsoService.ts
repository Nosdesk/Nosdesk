// A workspace's own sign-in provider for the requester portal.
import apiClient from '../apiClient'

export type RequesterSsoKind = 'entra' | 'google' | 'oidc'

export interface RequesterSsoProvider {
  kind: RequesterSsoKind
  display_name: string
  issuer_url: string
  client_id: string
  has_client_secret: boolean
  allowed_domains: string[]
  enabled: boolean
}

export interface RequesterSsoState {
  provider: RequesterSsoProvider | null
  /** Register this with the provider as the redirect URI. */
  redirect_uri: string | null
  /** Any OpenID Connect provider (self-hosted only). */
  generic_oidc_allowed: boolean
}

export interface RequesterSsoInput {
  kind: RequesterSsoKind
  display_name: string | null
  tenant_id: string | null
  issuer_url: string | null
  client_id: string
  /** Omit to keep the stored secret. */
  client_secret: string | null
  allowed_domains: string[]
  enabled: boolean
}

export const requesterSsoService = {
  async get(): Promise<RequesterSsoState> {
    const { data } = await apiClient.get<RequesterSsoState>('/admin/requester-sso')
    return data
  },
  async save(input: RequesterSsoInput): Promise<RequesterSsoProvider> {
    const { data } = await apiClient.put<{ provider: RequesterSsoProvider }>('/admin/requester-sso', input)
    return data.provider
  },
  async remove(): Promise<void> {
    await apiClient.delete('/admin/requester-sso')
  },
}

/** The Entra tenant from a stored issuer (`…/{tenant}/v2.0`). */
export function entraTenant(issuer: string): string {
  const m = issuer.match(/login\.microsoftonline\.com\/([^/]+)\/v2\.0/)
  return m?.[1] ?? ''
}
