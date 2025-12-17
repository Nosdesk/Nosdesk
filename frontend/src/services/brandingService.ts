import apiClient from './apiConfig'
import { logger } from '@/utils/logger'

export interface BrandingConfig {
  app_name: string
  logo_url: string | null
  logo_light_url: string | null
  favicon_url: string | null
  primary_color: string | null
  updated_at: string | null
}

export interface UpdateBrandingRequest {
  app_name?: string
  primary_color?: string | null
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
        updated_at: null
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

  /**
   * Apply branding to the document (favicon and title)
   */
  applyBranding(config: BrandingConfig): void {
    // Update page title
    if (config.app_name) {
      // Update any existing title to include the app name
      const currentTitle = document.title
      if (!currentTitle.includes(config.app_name)) {
        document.title = config.app_name
      }
    }

    // Update favicon
    if (config.favicon_url) {
      this.updateFavicon(config.favicon_url)
    }
  }

  /**
   * Update the favicon dynamically
   */
  updateFavicon(url: string): void {
    // Remove existing favicon links
    const existingIcons = document.querySelectorAll('link[rel*="icon"]')
    existingIcons.forEach(icon => icon.remove())

    // Create new favicon link
    const link = document.createElement('link')
    link.rel = 'icon'

    // Determine type from URL
    if (url.endsWith('.svg')) {
      link.type = 'image/svg+xml'
    } else if (url.endsWith('.ico')) {
      link.type = 'image/x-icon'
    } else if (url.endsWith('.png')) {
      link.type = 'image/png'
    }

    link.href = url
    document.head.appendChild(link)

    // Also add alternate icon for legacy browsers
    if (!url.endsWith('.ico')) {
      const alternateLink = document.createElement('link')
      alternateLink.rel = 'alternate icon'
      alternateLink.href = url
      document.head.appendChild(alternateLink)
    }
  }

  /**
   * Reset favicon to default
   */
  resetFavicon(): void {
    const existingIcons = document.querySelectorAll('link[rel*="icon"]')
    existingIcons.forEach(icon => icon.remove())

    // Restore default favicons
    const defaultIcons = [
      { rel: 'icon', type: 'image/svg+xml', href: '/favicon.svg' },
      { rel: 'icon', type: 'image/png', sizes: '32x32', href: '/favicon-32.png' },
      { rel: 'alternate icon', href: '/favicon.ico' }
    ]

    defaultIcons.forEach(iconData => {
      const link = document.createElement('link')
      link.rel = iconData.rel
      if (iconData.type) link.type = iconData.type
      if (iconData.sizes) link.setAttribute('sizes', iconData.sizes)
      link.href = iconData.href
      document.head.appendChild(link)
    })
  }
}

export default new BrandingService()
