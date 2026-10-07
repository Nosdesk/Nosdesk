import apiClient from '../apiClient'
import { logger } from '../utils/logger'

/**
 * What a page displays: the public branding routes return only this, and the
 * branding store and its cache hold only this.
 */
export interface BrandingConfig {
  app_name: string
  logo_url: string | null
  logo_light_url: string | null
  favicon_url: string | null
  primary_color: string | null
  updated_at: string | null
}

/** The workspace's branding settings, for the admin forms that edit them. */
export interface AdminBrandingConfig extends BrandingConfig {
  /**
   * Workspace-wide default email signature. `null` = no org default
   * (the outbound pipeline sends agents' replies unsigned when they
   * also have no personal signature). Empty string is sent to the
   * backend to clear it back to `null`.
   */
  signature_default: string | null
  /**
   * Whether to send the "we got your message" auto-acknowledgement
   * when a channel message opens a new ticket.
   */
  channel_auto_ack_enabled: boolean
  /**
   * Admin-overridden auto-ack body. `null` = use the built-in FTL
   * default for the resolved locale. Empty string is sent to clear
   * back to `null`.
   */
  channel_auto_ack_template: string | null
  /**
   * Whether the anti-phishing security note renders in the email
   * footer. Off by default until an admin enables it.
   */
  email_security_note_enabled: boolean
  /**
   * Admin-overridden security-note body. `null` = use the built-in
   * localized default. Empty string is sent to clear back to `null`.
   */
  email_security_note_template: string | null
}

export interface EmailPreview {
  light: string
  dark: string
}

export interface UpdateBrandingRequest {
  app_name?: string
  primary_color?: string | null
  signature_default?: string | null
  channel_auto_ack_enabled?: boolean
  channel_auto_ack_template?: string | null
  email_security_note_enabled?: boolean
  email_security_note_template?: string | null
}

class BrandingService {
  /**
   * Get branding configuration (public endpoint)
   */
  async getBrandingConfig(): Promise<BrandingConfig> {
    try {
      const response = await apiClient.get<BrandingConfig>('/branding')
      return response.data
    } catch (error) {
      logger.error('Error fetching branding config:', error)
      // Return defaults if fetch fails
      return {
        app_name: 'Nosdesk',
        logo_url: null,
        logo_light_url: null,
        favicon_url: null,
        primary_color: null,
        updated_at: null
      }
    }
  }

  /**
   * The selected workspace's branding, for a signed-in member of the
   * single-origin agent app, where `getBrandingConfig`'s public route has no
   * workspace to resolve (it goes by the Host). A failure throws, so the
   * caller keeps the branding it has instead of repainting the defaults.
   */
  async getWorkspaceBranding(): Promise<BrandingConfig> {
    const response = await apiClient.get<BrandingConfig>('/workspace/branding')
    return response.data
  }

  /**
   * The workspace's branding settings, for the admin forms that edit them.
   * Unlike `getBrandingConfig`, a failure throws: a form seeded from the
   * defaults would show the workspace's settings as unset.
   */
  async getAdminBrandingConfig(): Promise<AdminBrandingConfig> {
    const response = await apiClient.get<AdminBrandingConfig>('/admin/branding/config')
    return response.data
  }

  /**
   * Update branding configuration (admin only)
   */
  async updateBrandingConfig(update: UpdateBrandingRequest): Promise<AdminBrandingConfig> {
    try {
      const response = await apiClient.patch<AdminBrandingConfig>('/admin/branding/config', update)
      return response.data
    } catch (error) {
      logger.error('Error updating branding config:', error)
      throw error
    }
  }

  /**
   * Upload branding image (logo or favicon)
   */
  async uploadBrandingImage(
    file: File,
    type: 'logo' | 'logo_light' | 'favicon'
  ): Promise<{ url: string; settings: AdminBrandingConfig }> {
    try {
      const formData = new FormData()
      formData.append('file', file)

      const response = await apiClient.post<{ status: string; url: string; settings: AdminBrandingConfig }>(
        `/admin/branding/image?type=${type}`,
        formData,
        {
          headers: {
            'Content-Type': 'multipart/form-data'
          }
        }
      )

      return {
        url: response.data.url,
        settings: response.data.settings
      }
    } catch (error) {
      logger.error('Error uploading branding image:', error)
      throw error
    }
  }

  /**
   * Delete branding image
   */
  async deleteBrandingImage(type: 'logo' | 'logo_light' | 'favicon'): Promise<AdminBrandingConfig> {
    try {
      const response = await apiClient.delete<{ status: string; settings: AdminBrandingConfig }>(
        `/admin/branding/image?type=${type}`
      )

      return response.data.settings
    } catch (error) {
      logger.error('Error deleting branding image:', error)
      throw error
    }
  }

  /**
   * The test email as recipients see it, on the light and the dark paper
   * (admin only). Each is a complete HTML document for a sandboxed frame.
   */
  async getEmailPreview(): Promise<EmailPreview> {
    const response = await apiClient.get<EmailPreview>('/admin/branding/email-preview')
    return response.data
  }

  // Note: Favicon management is now handled by useFavicon composable in App.vue
  // This provides reactive updates when brandingStore.faviconUrl changes
}

export default new BrandingService()
