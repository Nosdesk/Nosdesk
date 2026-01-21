<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import BackButton from '@/components/common/BackButton.vue'
import AlertMessage from '@/components/common/AlertMessage.vue'
import LoadingSpinner from '@/components/common/LoadingSpinner.vue'
import ColorHueSlider from '@/components/common/ColorHueSlider.vue'
import brandingService, { type BrandingConfig } from '@/services/brandingService'
import uploadService from '@/services/uploadService'
import { useBrandingStore } from '@/stores/branding'

// Get the branding store to update it when settings change
const brandingStore = useBrandingStore()

// State
const isLoading = ref(false)
const isSaving = ref(false)
const errorMessage = ref('')
const successMessage = ref('')
const brandingConfig = ref<BrandingConfig | null>(null)

// Form state
const appName = ref('Nosdesk')
const primaryColor = ref('')

// File input refs
const logoInput = ref<HTMLInputElement | null>(null)
const logoLightInput = ref<HTMLInputElement | null>(null)
const faviconInput = ref<HTMLInputElement | null>(null)

// Upload states
const uploadingLogo = ref(false)
const uploadingLogoLight = ref(false)
const uploadingFavicon = ref(false)

// Computed
const isConfigured = computed(() => {
  return (
    brandingConfig.value?.logo_url ||
    brandingConfig.value?.favicon_url ||
    brandingConfig.value?.primary_color ||
    brandingConfig.value?.app_name !== 'Nosdesk'
  )
})

// Load branding configuration
const loadBrandingConfig = async () => {
  isLoading.value = true
  errorMessage.value = ''

  try {
    const config = await brandingService.getBrandingConfig()
    brandingConfig.value = config
    appName.value = config.app_name || 'Nosdesk'
    primaryColor.value = config.primary_color || ''
  } catch (error) {
    console.error('Failed to load branding configuration:', error)
    const axiosError = error as { response?: { data?: { message?: string } } }
    errorMessage.value = axiosError.response?.data?.message || 'Failed to load branding configuration'
  } finally {
    isLoading.value = false
  }
}

// Save app name and primary color
const saveSettings = async () => {
  isSaving.value = true
  errorMessage.value = ''
  successMessage.value = ''

  try {
    const config = await brandingService.updateBrandingConfig({
      app_name: appName.value,
      primary_color: primaryColor.value || null
    })
    brandingConfig.value = config
    successMessage.value = 'Branding settings saved successfully'

    // Update the branding store so changes reflect immediately across the app
    brandingStore.updateConfig(config)

    setTimeout(() => {
      successMessage.value = ''
    }, 3000)
  } catch (error) {
    console.error('Failed to save branding settings:', error)
    const axiosError = error as { response?: { data?: { message?: string } } }
    errorMessage.value = axiosError.response?.data?.message || 'Failed to save branding settings'
  } finally {
    isSaving.value = false
  }
}

// Handle logo upload
const handleLogoUpload = async (event: Event) => {
  const input = event.target as HTMLInputElement
  if (!input.files?.length) return

  const file = input.files[0]

  // Validate file
  const validation = uploadService.validateFile(file, {
    maxSizeMB: 2,
    allowedTypes: ['image/png', 'image/svg+xml', 'image/jpeg', 'image/webp']
  })

  if (!validation.valid) {
    errorMessage.value = validation.error || 'Invalid file'
    return
  }

  uploadingLogo.value = true
  errorMessage.value = ''

  try {
    const result = await brandingService.uploadBrandingImage(file, 'logo')
    brandingConfig.value = result.settings
    successMessage.value = 'Logo uploaded successfully'

    // Update the branding store so the logo reflects immediately
    brandingStore.updateConfig(result.settings)

    setTimeout(() => {
      successMessage.value = ''
    }, 3000)
  } catch (error) {
    console.error('Failed to upload logo:', error)
    const axiosError = error as { response?: { data?: { message?: string } } }
    errorMessage.value = axiosError.response?.data?.message || 'Failed to upload logo'
  } finally {
    uploadingLogo.value = false
    input.value = ''
  }
}

// Handle light theme logo upload
const handleLogoLightUpload = async (event: Event) => {
  const input = event.target as HTMLInputElement
  if (!input.files?.length) return

  const file = input.files[0]

  const validation = uploadService.validateFile(file, {
    maxSizeMB: 2,
    allowedTypes: ['image/png', 'image/svg+xml', 'image/jpeg', 'image/webp']
  })

  if (!validation.valid) {
    errorMessage.value = validation.error || 'Invalid file'
    return
  }

  uploadingLogoLight.value = true
  errorMessage.value = ''

  try {
    const result = await brandingService.uploadBrandingImage(file, 'logo_light')
    brandingConfig.value = result.settings
    successMessage.value = 'Light theme logo uploaded successfully'

    // Update the branding store so the logo reflects immediately
    brandingStore.updateConfig(result.settings)

    setTimeout(() => {
      successMessage.value = ''
    }, 3000)
  } catch (error) {
    console.error('Failed to upload light theme logo:', error)
    const axiosError = error as { response?: { data?: { message?: string } } }
    errorMessage.value = axiosError.response?.data?.message || 'Failed to upload light theme logo'
  } finally {
    uploadingLogoLight.value = false
    input.value = ''
  }
}

// Handle favicon upload
const handleFaviconUpload = async (event: Event) => {
  const input = event.target as HTMLInputElement
  if (!input.files?.length) return

  const file = input.files[0]

  const validation = uploadService.validateFile(file, {
    maxSizeMB: 2,
    allowedTypes: ['image/x-icon', 'image/vnd.microsoft.icon', 'image/png', 'image/svg+xml']
  })

  if (!validation.valid) {
    errorMessage.value = validation.error || 'Invalid file'
    return
  }

  uploadingFavicon.value = true
  errorMessage.value = ''

  try {
    const result = await brandingService.uploadBrandingImage(file, 'favicon')
    brandingConfig.value = result.settings
    successMessage.value = 'Favicon uploaded successfully'

    // Update the branding store so the favicon reflects immediately
    brandingStore.updateConfig(result.settings)

    setTimeout(() => {
      successMessage.value = ''
    }, 3000)
  } catch (error) {
    console.error('Failed to upload favicon:', error)
    const axiosError = error as { response?: { data?: { message?: string } } }
    errorMessage.value = axiosError.response?.data?.message || 'Failed to upload favicon'
  } finally {
    uploadingFavicon.value = false
    input.value = ''
  }
}

// Delete branding image
const deleteBrandingImage = async (type: 'logo' | 'logo_light' | 'favicon') => {
  errorMessage.value = ''
  successMessage.value = ''

  try {
    const config = await brandingService.deleteBrandingImage(type)
    brandingConfig.value = config

    // Update the branding store so changes reflect immediately
    brandingStore.updateConfig(config)

    const typeLabel = type === 'logo_light' ? 'Light theme logo' : type.charAt(0).toUpperCase() + type.slice(1)
    successMessage.value = `${typeLabel} removed successfully`

    setTimeout(() => {
      successMessage.value = ''
    }, 3000)
  } catch (error) {
    console.error(`Failed to delete ${type}:`, error)
    const axiosError = error as { response?: { data?: { message?: string } } }
    errorMessage.value = axiosError.response?.data?.message || `Failed to delete ${type}`
  }
}

onMounted(() => {
  loadBrandingConfig()
})
</script>

<template>
  <div class="flex-1">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/settings" label="Back to Administration" />
    </div>

    <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-4xl">
      <div class="mb-6">
        <h1 class="text-2xl font-bold text-primary">Branding</h1>
        <p class="text-secondary mt-2">
          Customize the appearance and branding of the application.
        </p>
      </div>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Error message -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Loading state -->
      <LoadingSpinner v-if="isLoading" text="Loading branding configuration..." />

      <!-- Branding configuration -->
      <div v-else class="flex flex-col gap-6">
        <!-- App Name and Primary Color -->
        <div class="bg-surface border border-default rounded-xl p-6 hover:border-strong transition-colors">
          <h2 class="text-lg font-semibold text-primary mb-4">General Settings</h2>

          <div class="flex flex-col gap-4">
            <!-- App Name -->
            <div class="flex flex-col gap-2">
              <label for="appName" class="text-sm font-medium text-primary">Application Name</label>
              <input
                id="appName"
                v-model="appName"
                type="text"
                class="bg-surface-alt border border-default rounded-lg px-3 py-2 text-primary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
                placeholder="Nosdesk"
              />
              <p class="text-xs text-tertiary">This name appears in the header and browser tab</p>
            </div>

            <!-- Primary Color -->
            <div class="flex flex-col gap-2">
              <ColorHueSlider v-model="primaryColor" label="Primary Color" />
              <p class="text-xs text-tertiary">Hex color code for accent elements (e.g., #2C80FF)</p>
            </div>

            <!-- Save Button -->
            <div class="flex justify-end pt-2">
              <button
                @click="saveSettings"
                :disabled="isSaving"
                class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 flex items-center gap-2"
              >
                <svg v-if="isSaving" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
                  <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                  <path class="opacity-75" fill="currentColor" d="m4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                </svg>
                {{ isSaving ? 'Saving...' : 'Save Settings' }}
              </button>
            </div>
          </div>
        </div>

        <!-- Logo Upload -->
        <div class="bg-surface border border-default rounded-xl p-6 hover:border-strong transition-colors">
          <h2 class="text-lg font-semibold text-primary mb-4">Logo</h2>

          <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
            <!-- Dark Theme Logo -->
            <div class="flex flex-col gap-3">
              <label class="text-sm font-medium text-primary">Dark Theme Logo</label>
              <div class="flex items-center gap-4">
                <div class="w-24 h-24 bg-surface-alt rounded-lg border border-default flex items-center justify-center overflow-hidden">
                  <img
                    v-if="brandingConfig?.logo_url"
                    :src="brandingConfig.logo_url"
                    alt="Logo"
                    class="max-w-full max-h-full object-contain"
                  />
                  <svg v-else class="w-12 h-12 text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z" />
                  </svg>
                </div>
                <div class="flex flex-col gap-2">
                  <input
                    ref="logoInput"
                    type="file"
                    accept="image/png,image/svg+xml,image/jpeg,image/webp"
                    class="hidden"
                    @change="handleLogoUpload"
                  />
                  <button
                    @click="logoInput?.click()"
                    :disabled="uploadingLogo"
                    class="px-3 py-1.5 text-sm bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50"
                  >
                    {{ uploadingLogo ? 'Uploading...' : 'Upload Logo' }}
                  </button>
                  <button
                    v-if="brandingConfig?.logo_url"
                    @click="deleteBrandingImage('logo')"
                    class="px-3 py-1.5 text-sm text-status-error hover:bg-status-error-muted rounded-lg transition-colors"
                  >
                    Remove
                  </button>
                </div>
              </div>
              <p class="text-xs text-tertiary">PNG, SVG, JPEG, or WebP. Max 2MB.</p>
            </div>

            <!-- Light Theme Logo -->
            <div class="flex flex-col gap-3">
              <label class="text-sm font-medium text-primary">Light Theme Logo (Optional)</label>
              <div class="flex items-center gap-4">
                <div class="w-24 h-24 bg-white rounded-lg border border-default flex items-center justify-center overflow-hidden">
                  <img
                    v-if="brandingConfig?.logo_light_url"
                    :src="brandingConfig.logo_light_url"
                    alt="Light Logo"
                    class="max-w-full max-h-full object-contain"
                  />
                  <img
                    v-else-if="brandingConfig?.logo_url"
                    :src="brandingConfig.logo_url"
                    alt="Logo"
                    class="max-w-full max-h-full object-contain opacity-50"
                  />
                  <svg v-else class="w-12 h-12 text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z" />
                  </svg>
                </div>
                <div class="flex flex-col gap-2">
                  <input
                    ref="logoLightInput"
                    type="file"
                    accept="image/png,image/svg+xml,image/jpeg,image/webp"
                    class="hidden"
                    @change="handleLogoLightUpload"
                  />
                  <button
                    @click="logoLightInput?.click()"
                    :disabled="uploadingLogoLight"
                    class="px-3 py-1.5 text-sm bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50"
                  >
                    {{ uploadingLogoLight ? 'Uploading...' : 'Upload Logo' }}
                  </button>
                  <button
                    v-if="brandingConfig?.logo_light_url"
                    @click="deleteBrandingImage('logo_light')"
                    class="px-3 py-1.5 text-sm text-status-error hover:bg-status-error-muted rounded-lg transition-colors"
                  >
                    Remove
                  </button>
                </div>
              </div>
              <p class="text-xs text-tertiary">Used when light theme is active. Falls back to main logo.</p>
            </div>
          </div>
        </div>

        <!-- Favicon Upload -->
        <div class="bg-surface border border-default rounded-xl p-6 hover:border-strong transition-colors">
          <h2 class="text-lg font-semibold text-primary mb-4">Favicon</h2>

          <div class="flex flex-col gap-3">
            <div class="flex items-center gap-4">
              <div class="w-16 h-16 bg-surface-alt rounded-lg border border-default flex items-center justify-center overflow-hidden">
                <img
                  v-if="brandingConfig?.favicon_url"
                  :src="brandingConfig.favicon_url"
                  alt="Favicon"
                  class="w-8 h-8 object-contain"
                />
                <svg v-else class="w-8 h-8 text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M5 3v4M3 5h4M6 17v4m-2-2h4m5-16l2.286 6.857L21 12l-5.714 2.143L13 21l-2.286-6.857L5 12l5.714-2.143L13 3z" />
                </svg>
              </div>
              <div class="flex flex-col gap-2">
                <input
                  ref="faviconInput"
                  type="file"
                  accept="image/x-icon,image/vnd.microsoft.icon,image/png,image/svg+xml"
                  class="hidden"
                  @change="handleFaviconUpload"
                />
                <button
                  @click="faviconInput?.click()"
                  :disabled="uploadingFavicon"
                  class="px-3 py-1.5 text-sm bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50"
                >
                  {{ uploadingFavicon ? 'Uploading...' : 'Upload Favicon' }}
                </button>
                <button
                  v-if="brandingConfig?.favicon_url"
                  @click="deleteBrandingImage('favicon')"
                  class="px-3 py-1.5 text-sm text-status-error hover:bg-status-error-muted rounded-lg transition-colors"
                >
                  Remove
                </button>
              </div>
            </div>
            <p class="text-xs text-tertiary">ICO, PNG, or SVG. Recommended size: 32x32 or 64x64 pixels.</p>
          </div>
        </div>

        <!-- Preview Section -->
        <div class="bg-surface border border-default rounded-xl p-6">
          <h2 class="text-lg font-semibold text-primary mb-4">Preview</h2>
          <div class="flex items-center gap-4 p-4 bg-surface-alt rounded-lg border border-default">
            <!-- Favicon preview -->
            <div class="w-8 h-8 bg-surface rounded border border-default flex items-center justify-center">
              <img
                v-if="brandingConfig?.favicon_url"
                :src="brandingConfig.favicon_url"
                alt="Favicon"
                class="w-4 h-4 object-contain"
              />
              <svg v-else class="w-4 h-4 text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 3v4M3 5h4M6 17v4m-2-2h4m5-16l2.286 6.857L21 12l-5.714 2.143L13 21l-2.286-6.857L5 12l5.714-2.143L13 3z" />
              </svg>
            </div>

            <!-- Logo preview -->
            <div class="h-10 flex items-center">
              <img
                v-if="brandingConfig?.logo_url"
                :src="brandingConfig.logo_url"
                alt="Logo"
                class="h-8 object-contain"
              />
              <span v-else class="text-lg font-semibold text-primary">{{ appName }}</span>
            </div>

            <!-- Separator -->
            <span class="text-tertiary">|</span>

            <!-- Primary color preview -->
            <div class="flex items-center gap-2">
              <div
                class="w-6 h-6 rounded-full border border-default"
                :style="{ backgroundColor: primaryColor || '#2C80FF' }"
              ></div>
              <span class="text-sm text-secondary">Primary Color</span>
            </div>
          </div>
        </div>

        <!-- Configuration status (only shown when custom branding is configured) -->
        <div
          v-if="isConfigured"
          class="p-4 rounded-lg border flex items-center gap-3 bg-status-success-muted border-status-success/30"
        >
          <svg
            class="w-5 h-5 text-status-success"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
          >
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
          <span class="text-status-success">Custom branding configured</span>
        </div>
      </div>
    </div>
  </div>
</template>
