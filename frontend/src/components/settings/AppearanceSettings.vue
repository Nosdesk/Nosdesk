<script setup lang="ts">
import { ref, watch, computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { useThemeStore } from '@/stores/theme'
import { useAuthStore } from '@/stores/auth'
import { getTheme } from '@/themes'
import type { ThemeMode } from '@/themes'
import ThemeCard from '@/components/settings/ThemeCard.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import ToggleSwitch from '@/components/common/ToggleSwitch.vue'
import Spinner from '@/components/common/Spinner.vue'
import userService from '@/services/userService'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const themeStore = useThemeStore()
const authStore = useAuthStore()

// Props
const props = defineProps<{
  targetUserUuid?: string
  targetUserTheme?: string | null
}>()

// Emits for notifications
const emit = defineEmits<{
  (e: 'success', message: string): void
  (e: 'error', message: string): void
}>()

// Whether we're editing another user's theme (admin mode)
const isAdminMode = computed(() => {
  return !!props.targetUserUuid && props.targetUserUuid !== authStore.user?.uuid
})

// Local reactive state
// In admin mode, initialize from the target user's saved theme; otherwise from the theme store
const showRedHorizonEasterEgg = ref(false)
const selectedTheme = ref<ThemeMode>(
  isAdminMode.value && props.targetUserTheme
    ? props.targetUserTheme as ThemeMode
    : themeStore.currentTheme
)
const compactView = ref(false)
const isUpdating = ref(false)

// Use computed with getter/setter for two-way binding with store
// Shows effective state (on for monochromatic themes) but sets user preference
const colorBlindMode = computed({
  get: () => themeStore.effectiveColorBlindMode,
  set: (value) => themeStore.setColorBlindMode(value)
})

// Asset-local theme mode - when enabled, theme is not synced to/from backend
const deviceLocalTheme = computed({
  get: () => themeStore.deviceLocalTheme,
  set: (value) => themeStore.setDeviceLocalTheme(value)
})

// Monochromatic themes that require colorblind mode
const MONOCHROMATIC_THEMES = ['epaper', 'red-horizon']
const isMonochromaticTheme = computed(() => {
  const effectiveId = themeStore.effectiveTheme?.meta?.id
  return MONOCHROMATIC_THEMES.includes(effectiveId)
})

// Watch theme store changes (only in self mode)
watch(
  () => themeStore.currentTheme,
  (newValue) => {
    if (!isAdminMode.value) {
      selectedTheme.value = newValue
    }
  }
)

// Update selectedTheme when target user data loads
watch(
  () => props.targetUserTheme,
  (newTheme) => {
    if (isAdminMode.value && newTheme) {
      selectedTheme.value = newTheme as ThemeMode
    }
  }
)

// Get the UUID to update (target user or current user)
const userUuid = computed(() => {
  return props.targetUserUuid || authStore.user?.uuid
})

// Handle theme selection
const selectTheme = async (themeId: ThemeMode) => {
  if (selectedTheme.value === themeId) return

  const previousTheme = selectedTheme.value
  isUpdating.value = true
  selectedTheme.value = themeId

  if (isAdminMode.value) {
    // Admin mode: update target user's theme on backend without changing admin's local theme
    try {
      await userService.updateUser(props.targetUserUuid!, { theme: themeId })
      const themeName = themeId === 'system' ? t('settings-appearance-system-theme-name') : (getTheme(themeId)?.meta.name || themeId)
      emit('success', t('settings-appearance-theme-changed', { name: themeName }))
      showRedHorizonEasterEgg.value = themeId === 'red-horizon'
    } catch {
      emit('error', t('settings-appearance-theme-save-failed'))
      selectedTheme.value = previousTheme
    }
  } else {
    // Self mode: update local theme and sync to backend
    themeStore.setTheme(themeId)

    if (userUuid.value && !deviceLocalTheme.value) {
      const success = await themeStore.syncThemeToBackend(userUuid.value)

      if (success) {
        const themeName = themeId === 'system' ? t('settings-appearance-system-theme-name') : themeStore.effectiveTheme.meta.name
        emit('success', t('settings-appearance-theme-changed', { name: themeName }))
      } else {
        emit('error', t('settings-appearance-theme-save-failed'))
        selectedTheme.value = previousTheme
        themeStore.setTheme(previousTheme)
      }
    } else {
      const themeName = themeId === 'system' ? t('settings-appearance-system-theme-name') : themeStore.effectiveTheme.meta.name
      const key = deviceLocalTheme.value
        ? 'settings-appearance-theme-changed-device-only'
        : 'settings-appearance-theme-changed'
      emit('success', t(key, { name: themeName }))
    }
  }

  isUpdating.value = false
}

// Handle color blind mode toggle
const handleColorBlindModeToggle = () => {
  emit('success', t('settings-appearance-colorblind-toggled', { state: colorBlindMode.value ? 'enabled' : 'disabled' }))
}

// Handle device local theme toggle
const handleDeviceLocalThemeToggle = () => {
  emit('success', t('settings-appearance-device-local-toggled', { state: deviceLocalTheme.value ? 'enabled' : 'disabled' }))
}

// Handle compact view toggle
const handleCompactViewToggle = () => {
  emit('success', t('settings-appearance-compact-toggled', { state: compactView.value ? 'enabled' : 'disabled' }))
}
</script>

<template>
  <SectionCard content-padding="p-4 sm:p-6">
    <template #leading>
      <!-- Custom palette/brush glyph; not a registry action icon (decorative section badge). -->
      <svg
        xmlns="http://www.w3.org/2000/svg"
        class="h-4 w-4 text-accent flex-shrink-0"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
      >
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          stroke-width="2"
          d="M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zm0 0h12a2 2 0 002-2v-4a2 2 0 00-2-2h-2.343M11 7.343l1.657-1.657a2 2 0 012.828 0l2.829 2.829a2 2 0 010 2.828l-8.486 8.485M7 17h.01"
        />
      </svg>
    </template>
    <template #title>{{ t('settings-appearance-title') }}</template>
    <template #headerActions>
      <Spinner v-if="isUpdating || themeStore.isSyncing" class="text-accent" />
    </template>

    <div class="flex flex-col gap-6">
      <!-- Theme Selection -->
      <div class="flex flex-col gap-4">
        <div class="flex items-center justify-between">
          <div>
            <h3 class="text-sm font-medium text-primary">{{ t('settings-appearance-theme-heading') }}</h3>
            <p class="text-xs text-tertiary mt-0.5">{{ t('settings-appearance-theme-description') }}</p>
          </div>
        </div>

        <!-- Asset-only Theme Toggle (self mode only) -->
        <ToggleSwitch
          v-if="!isAdminMode"
          v-model="deviceLocalTheme"
          :label="t('settings-appearance-device-local-label')"
          :description="t('settings-appearance-device-local-description')"
          @update:modelValue="handleDeviceLocalThemeToggle"
        />

        <!-- System Theme Option -->
        <div>
          <h4 class="text-xs font-medium text-tertiary uppercase tracking-wider mb-2">
            {{ t('settings-appearance-section-automatic') }}
          </h4>
          <div class="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 gap-2">
            <ThemeCard
              :is-system="true"
              :selected="selectedTheme === 'system'"
              :disabled="isUpdating || themeStore.isSyncing"
              @select="selectTheme('system')"
            />
          </div>
        </div>

        <!-- Light Themes -->
        <div>
          <h4 class="text-xs font-medium text-tertiary uppercase tracking-wider mb-2">
            {{ t('settings-appearance-section-light') }}
          </h4>
          <div class="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 gap-2">
            <ThemeCard
              v-for="theme in themeStore.lightThemes"
              :key="theme.meta.id"
              :theme="theme"
              :selected="selectedTheme === theme.meta.id"
              :disabled="isUpdating || themeStore.isSyncing"
              @select="selectTheme(theme.meta.id)"
            />
          </div>
        </div>

        <!-- Dark Themes -->
        <div>
          <h4 class="text-xs font-medium text-tertiary uppercase tracking-wider mb-2">
            {{ t('settings-appearance-section-dark') }}
          </h4>
          <div class="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 gap-2">
            <ThemeCard
              v-for="theme in themeStore.darkThemes"
              :key="theme.meta.id"
              :theme="theme"
              :selected="selectedTheme === theme.meta.id"
              :disabled="isUpdating || themeStore.isSyncing"
              @select="selectTheme(theme.meta.id)"
            />
          </div>
        </div>
        <!-- Red Horizon easter egg (admin mode only) -->
        <Transition name="fade">
          <div
            v-if="showRedHorizonEasterEgg"
            class="flex items-center gap-2 p-3 bg-status-error/10 border border-status-error/30 rounded-lg"
          >
            <span class="text-sm text-status-error">{{ t('settings-appearance-red-horizon-easter-egg') }}</span>
          </div>
        </Transition>
      </div>

      <!-- Accessibility Options (self mode only) -->
      <div v-if="!isAdminMode" class="flex flex-col gap-4 pt-2 border-t border-default">
        <div>
          <h3 class="text-sm font-medium text-primary">{{ t('settings-appearance-accessibility-heading') }}</h3>
          <p class="text-xs text-tertiary mt-0.5">{{ t('settings-appearance-accessibility-description') }}</p>
        </div>

        <!-- Color Blind Friendly Mode Toggle -->
        <ToggleSwitch
          v-model="colorBlindMode"
          :label="t('settings-appearance-colorblind-label')"
          :description="isMonochromaticTheme
            ? t('settings-appearance-colorblind-description-monochrome')
            : t('settings-appearance-colorblind-description-default')"
          :disabled="isMonochromaticTheme"
          @update:modelValue="handleColorBlindModeToggle"
        />
      </div>

      <!-- Display Options (self mode only) -->
      <div v-if="!isAdminMode" class="flex flex-col gap-4 pt-2 border-t border-default">
        <div>
          <h3 class="text-sm font-medium text-primary">{{ t('settings-appearance-display-heading') }}</h3>
          <p class="text-xs text-tertiary mt-0.5">{{ t('settings-appearance-display-description') }}</p>
        </div>

        <!-- Compact View Toggle -->
        <ToggleSwitch
          v-model="compactView"
          :label="t('settings-appearance-compact-label')"
          :description="t('settings-appearance-compact-description')"
          @update:modelValue="handleCompactViewToggle"
        />
      </div>
    </div>
  </SectionCard>
</template>

<style scoped>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.3s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
