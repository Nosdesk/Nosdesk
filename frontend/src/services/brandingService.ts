import apiClient from '@nosdesk/core/apiClient'
import { logger } from '@/utils/logger'

export interface BrandingConfig {
  app_name: string
  logo_url: string | null
  logo_light_url: string | null
  favicon_url: string | null
  primary_color: string | null
  updated_at: string | null
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
  private cachedConfig: BrandingConfig | null = null

  /**
   * Get branding configuration (public endpoint)
   */
  async getBrandingConfig(): Promise<BrandingConfig> {
    try {
      const response = await apiClient.get<BrandingConfig>('/branding')
      this.cachedConfig = response.data
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
        updated_at: null,
        signature_default: null,
        channel_auto_ack_enabled: true,
        channel_auto_ack_template: null,
        email_security_note_enabled: false,
        email_security_note_template: null
      }
    }
  }

  /**
   * Get cached branding config (for synchronous access)
   */
  getCachedConfig(): BrandingConfig | null {
    return this.cachedConfig
  }

  /**
   * Update branding configuration (admin only)
   */
  async updateBrandingConfig(update: UpdateBrandingRequest): Promise<BrandingConfig> {
    try {
      const response = await apiClient.patch<BrandingConfig>('/admin/branding/config', update)
      this.cachedConfig = response.data
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
  ): Promise<{ url: string; settings: BrandingConfig }> {
    try {
      const formData = new FormData()
      formData.append('file', file)

      const response = await apiClient.post<{ status: string; url: string; settings: BrandingConfig }>(
        `/admin/branding/image?type=${type}`,
        formData,
        {
          headers: {
            'Content-Type': 'multipart/form-data'
          }
        }
      )

      if (response.data.settings) {
        this.cachedConfig = response.data.settings
      }

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
  async deleteBrandingImage(type: 'logo' | 'logo_light' | 'favicon'): Promise<BrandingConfig> {
    try {
      const response = await apiClient.delete<{ status: string; settings: BrandingConfig }>(
        `/admin/branding/image?type=${type}`
      )

      if (response.data.settings) {
        this.cachedConfig = response.data.settings
      }

      return response.data.settings
    } catch (error) {
      logger.error('Error deleting branding image:', error)
      throw error
    }
  }

  // Note: Favicon management is now handled by useFavicon composable in App.vue
  // This provides reactive updates when brandingStore.faviconUrl changes
}

export default new BrandingService()
