<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useFluent } from 'fluent-vue'

import AlertMessage from '@/components/common/AlertMessage.vue'
import LoadingSpinner from '@/components/common/LoadingSpinner.vue'
import Icon from '@/components/common/Icon.vue'
import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'
import ColorHueSlider from '@/components/common/ColorHueSlider.vue'
import brandingService, { type BrandingConfig } from '@/services/brandingService'
import uploadService from '@/services/uploadService'
import { useBrandingStore } from '@/stores/branding'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import { extractErrorMessage } from '@/utils/errors'

// Get the branding store to update it when settings change
const brandingStore = useBrandingStore()
const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

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
    errorMessage.value = extractErrorMessage(error, t('admin-branding-error-load'))
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
    successMessage.value = t('admin-branding-success-saved')

    // Update the branding store so changes reflect immediately across the app
    brandingStore.updateConfig(config)

    setTimeout(() => {
      successMessage.value = ''
    }, 3000)
  } catch (error) {
    console.error('Failed to save branding settings:', error)
    errorMessage.value = extractErrorMessage(error, t('admin-branding-error-save'))
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
    errorMessage.value = validation.error || t('admin-branding-error-invalid-file')
    return
  }

  uploadingLogo.value = true
  errorMessage.value = ''

  try {
    const result = await brandingService.uploadBrandingImage(file, 'logo')
    brandingConfig.value = result.settings
    successMessage.value = t('admin-branding-success-logo')

    // Update the branding store so the logo reflects immediately
    brandingStore.updateConfig(result.settings)

    setTimeout(() => {
      successMessage.value = ''
    }, 3000)
  } catch (error) {
    console.error('Failed to upload logo:', error)
    errorMessage.value = extractErrorMessage(error, t('admin-branding-error-upload-logo'))
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
    errorMessage.value = validation.error || t('admin-branding-error-invalid-file')
    return
  }

  uploadingLogoLight.value = true
  errorMessage.value = ''

  try {
    const result = await brandingService.uploadBrandingImage(file, 'logo_light')
    brandingConfig.value = result.settings
    successMessage.value = t('admin-branding-success-logo-light')

    // Update the branding store so the logo reflects immediately
    brandingStore.updateConfig(result.settings)

    setTimeout(() => {
      successMessage.value = ''
    }, 3000)
  } catch (error) {
    console.error('Failed to upload light theme logo:', error)
    errorMessage.value = extractErrorMessage(error, t('admin-branding-error-upload-logo-light'))
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
    errorMessage.value = validation.error || t('admin-branding-error-invalid-file')
    return
  }

  uploadingFavicon.value = true
  errorMessage.value = ''

  try {
    const result = await brandingService.uploadBrandingImage(file, 'favicon')
    brandingConfig.value = result.settings
    successMessage.value = t('admin-branding-success-favicon')

    // Update the branding store so the favicon reflects immediately
    brandingStore.updateConfig(result.settings)

    setTimeout(() => {
      successMessage.value = ''
    }, 3000)
  } catch (error) {
    console.error('Failed to upload favicon:', error)
    errorMessage.value = extractErrorMessage(error, t('admin-branding-error-upload-favicon'))
  } finally {
    uploadingFavicon.value = false
    input.value = ''
  }
}

// Delete branding image. Wrapped with a confirmation modal —
// branding assets are easy to lose accidentally (one stray click on
// "Remove") and re-uploading is friction the operator wouldn't
// expect from a single button click.
type BrandingImageType = 'logo' | 'logo_light' | 'favicon'

const pendingDelete = ref<BrandingImageType | null>(null)

const assetLabel = (type: BrandingImageType): string => {
  if (type === 'logo_light') return t('admin-branding-asset-logo-light')
  if (type === 'favicon') return t('admin-branding-asset-favicon')
  return t('admin-branding-asset-logo')
}

const pendingDeleteLabel = computed(() =>
  pendingDelete.value ? assetLabel(pendingDelete.value) : ''
)

function requestDeleteBrandingImage(type: BrandingImageType): void {
  pendingDelete.value = type
}

async function confirmDeleteBrandingImage(): Promise<void> {
  const type = pendingDelete.value
  if (!type) return
  pendingDelete.value = null

  errorMessage.value = ''
  successMessage.value = ''

  try {
    const config = await brandingService.deleteBrandingImage(type)
    brandingConfig.value = config

    // Update the branding store so changes reflect immediately
    brandingStore.updateConfig(config)

    successMessage.value = t('admin-branding-success-removed', { asset: assetLabel(type) })

    setTimeout(() => {
      successMessage.value = ''
    }, 3000)
  } catch (error) {
    console.error(`Failed to delete ${type}:`, error)
    errorMessage.value = extractErrorMessage(error, t('admin-branding-error-delete', { asset: assetLabel(type) }))
  }
}

onMounted(() => {
  loadBrandingConfig()
})
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-6xl">
      <div class="mb-6">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('admin-branding-title') }}</h1>
        <p class="text-secondary mt-2">
          {{ $t('admin-branding-description') }}
        </p>
      </div>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Error message -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Loading state -->
      <LoadingSpinner v-if="isLoading" :text="$t('admin-branding-loading')" />

      <!-- Branding configuration -->
      <div v-else class="flex flex-col gap-6">
        <!-- App Name and Primary Color -->
        <div class="bg-surface border border-default rounded-xl p-6 hover:border-strong transition-colors">
          <h2 class="text-lg font-semibold text-primary mb-4">{{ $t('admin-branding-general-heading') }}</h2>

          <div class="flex flex-col gap-4">
            <!-- App Name -->
            <div class="flex flex-col gap-2">
              <label for="appName" class="text-sm font-medium text-primary">{{ $t('admin-branding-app-name-label') }}</label>
              <FormInput
                id="appName"
                v-model="appName"
                :placeholder="$t('admin-branding-app-name-placeholder')"
              />
              <p class="text-xs text-tertiary">{{ $t('admin-branding-app-name-hint') }}</p>
            </div>

            <!-- Primary Color -->
            <div class="flex flex-col gap-2">
              <ColorHueSlider v-model="primaryColor" :label="$t('admin-branding-primary-color-label')" />
              <p class="text-xs text-tertiary">{{ $t('admin-branding-primary-color-hint') }}</p>
            </div>

            <!-- Save Button -->
            <div class="flex justify-end pt-2">
              <Button :loading="isSaving" @click="saveSettings">
                {{ isSaving ? $t('admin-branding-saving') : $t('admin-branding-save') }}
              </Button>
            </div>
          </div>
        </div>

        <!-- Logo Upload -->
        <div class="bg-surface border border-default rounded-xl p-6 hover:border-strong transition-colors">
          <h2 class="text-lg font-semibold text-primary mb-4">{{ $t('admin-branding-logo-heading') }}</h2>

          <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
            <!-- Dark Theme Logo -->
            <div class="flex flex-col gap-3">
              <label class="text-sm font-medium text-primary">{{ $t('admin-branding-logo-dark-label') }}</label>
              <div class="flex items-center gap-4">
                <div class="w-24 h-24 bg-surface-alt rounded-lg border border-default flex items-center justify-center overflow-hidden">
                  <img
                    v-if="brandingConfig?.logo_url"
                    :src="brandingConfig.logo_url"
                    :alt="t('admin-branding-aria-logo')"
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
                  <Button size="sm" :loading="uploadingLogo" @click="logoInput?.click()">
                    {{ uploadingLogo ? $t('admin-branding-logo-uploading') : $t('admin-branding-logo-upload') }}
                  </Button>
                  <Button
                    v-if="brandingConfig?.logo_url"
                    variant="ghost-danger"
                    size="sm"
                    @click="requestDeleteBrandingImage('logo')"
                  >
                    {{ $t('admin-branding-logo-remove') }}
                  </Button>
                </div>
              </div>
              <p class="text-xs text-tertiary">{{ $t('admin-branding-logo-formats') }}</p>
            </div>

            <!-- Light Theme Logo -->
            <div class="flex flex-col gap-3">
              <label class="text-sm font-medium text-primary">{{ $t('admin-branding-logo-light-label') }}</label>
              <div class="flex items-center gap-4">
                <div class="w-24 h-24 bg-white rounded-lg border border-default flex items-center justify-center overflow-hidden">
                  <img
                    v-if="brandingConfig?.logo_light_url"
                    :src="brandingConfig.logo_light_url"
                    :alt="t('admin-branding-aria-logo-light')"
                    class="max-w-full max-h-full object-contain"
                  />
                  <img
                    v-else-if="brandingConfig?.logo_url"
                    :src="brandingConfig.logo_url"
                    :alt="t('admin-branding-aria-logo')"
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
                  <Button size="sm" :loading="uploadingLogoLight" @click="logoLightInput?.click()">
                    {{ uploadingLogoLight ? $t('admin-branding-logo-uploading') : $t('admin-branding-logo-upload') }}
                  </Button>
                  <Button
                    v-if="brandingConfig?.logo_light_url"
                    variant="ghost-danger"
                    size="sm"
                    @click="requestDeleteBrandingImage('logo_light')"
                  >
                    {{ $t('admin-branding-logo-remove') }}
                  </Button>
                </div>
              </div>
              <p class="text-xs text-tertiary">{{ $t('admin-branding-logo-light-hint') }}</p>
            </div>
          </div>
        </div>

        <!-- Favicon Upload -->
        <div class="bg-surface border border-default rounded-xl p-6 hover:border-strong transition-colors">
          <h2 class="text-lg font-semibold text-primary mb-4">{{ $t('admin-branding-favicon-heading') }}</h2>

          <div class="flex flex-col gap-3">
            <div class="flex items-center gap-4">
              <div class="w-16 h-16 bg-surface-alt rounded-lg border border-default flex items-center justify-center overflow-hidden">
                <img
                  v-if="brandingConfig?.favicon_url"
                  :src="brandingConfig.favicon_url"
                  :alt="t('admin-branding-aria-favicon')"
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
                <Button size="sm" :loading="uploadingFavicon" @click="faviconInput?.click()">
                  {{ uploadingFavicon ? $t('admin-branding-favicon-uploading') : $t('admin-branding-favicon-upload') }}
                </Button>
                <Button
                  v-if="brandingConfig?.favicon_url"
                  variant="ghost-danger"
                  size="sm"
                  @click="requestDeleteBrandingImage('favicon')"
                >
                  {{ $t('admin-branding-logo-remove') }}
                </Button>
              </div>
            </div>
            <p class="text-xs text-tertiary">{{ $t('admin-branding-favicon-formats') }}</p>
          </div>
        </div>

        <!-- Preview Section -->
        <div class="bg-surface border border-default rounded-xl p-6">
          <h2 class="text-lg font-semibold text-primary mb-4">{{ $t('admin-branding-preview-heading') }}</h2>
          <div class="flex items-center gap-4 p-4 bg-surface-alt rounded-lg border border-default">
            <!-- Favicon preview -->
            <div class="w-8 h-8 bg-surface rounded border border-default flex items-center justify-center">
              <img
                v-if="brandingConfig?.favicon_url"
                :src="brandingConfig.favicon_url"
                :alt="t('admin-branding-aria-favicon')"
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
                :alt="t('admin-branding-aria-logo')"
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
              <span class="text-sm text-secondary">{{ $t('admin-branding-primary-color-preview') }}</span>
            </div>
          </div>
        </div>

        <!-- Configuration status (only shown when custom branding is configured) -->
        <div
          v-if="isConfigured"
          class="p-4 rounded-lg border flex items-center gap-3 bg-status-success-muted border-status-success/30"
        >
          <Icon name="checkCircle" size="md" class="text-status-success" />
          <span class="text-status-success">{{ $t('admin-branding-configured') }}</span>
        </div>
      </div>
    </div>

    <ConfirmModal
      :show="pendingDelete !== null"
      variant="danger"
      :title="t('admin-branding-confirm-title', { asset: pendingDeleteLabel })"
      :message="$t('admin-branding-confirm-message')"
      :confirm-label="$t('admin-branding-confirm-remove')"
      @confirm="confirmDeleteBrandingImage"
      @close="pendingDelete = null"
    />
  </div>
</template>
