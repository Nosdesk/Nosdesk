<script setup lang="ts">
import { ref, watch, computed } from 'vue'
import { useThemeStore } from '@/stores/theme'
import { useAuthStore } from '@/stores/auth'
import { getTheme } from '@/themes'
import type { ThemeMode } from '@/themes'
import ThemeCard from '@/components/settings/ThemeCard.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import ToggleSwitch from '@/components/common/ToggleSwitch.vue'
import Spinner from '@/components/common/Spinner.vue'
import userService from '@/services/userService'

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

// Device-local theme mode - when enabled, theme is not synced to/from backend
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
      const themeName = themeId === 'system' ? 'System' : (getTheme(themeId)?.meta.name || themeId)
      emit('success', `Theme changed to ${themeName}`)
      showRedHorizonEasterEgg.value = themeId === 'red-horizon'
    } catch {
      emit('error', 'Failed to save theme preference')
      selectedTheme.value = previousTheme
    }
  } else {
    // Self mode: update local theme and sync to backend
    themeStore.setTheme(themeId)

    if (userUuid.value && !deviceLocalTheme.value) {
      const success = await themeStore.syncThemeToBackend(userUuid.value)

      if (success) {
        const themeName = themeId === 'system' ? 'System' : themeStore.effectiveTheme.meta.name
        emit('success', `Theme changed to ${themeName}`)
      } else {
        emit('error', 'Failed to save theme preference')
        selectedTheme.value = previousTheme
        themeStore.setTheme(previousTheme)
      }
    } else {
      const themeName = themeId === 'system' ? 'System' : themeStore.effectiveTheme.meta.name
      emit('success', `Theme changed to ${themeName}${deviceLocalTheme.value ? ' (device only)' : ''}`)
    }
  }

  isUpdating.value = false
}

// Handle color blind mode toggle
const handleColorBlindModeToggle = () => {
  emit('success', `Color blind friendly mode ${colorBlindMode.value ? 'enabled' : 'disabled'}`)
}

// Handle device local theme toggle
const handleDeviceLocalThemeToggle = () => {
  emit('success', `Device-only theme ${deviceLocalTheme.value ? 'enabled' : 'disabled'}`)
}

// Handle compact view toggle
const handleCompactViewToggle = () => {
  emit('success', `Compact view ${compactView.value ? 'enabled' : 'disabled'}`)
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
    <template #title>Appearance</template>
    <template #headerActions>
      <Spinner v-if="isUpdating || themeStore.isSyncing" class="text-accent" />
    </template>

    <div class="flex flex-col gap-6">
      <!-- Theme Selection -->
      <div class="flex flex-col gap-4">
        <div class="flex items-center justify-between">
          <div>
            <h3 class="text-sm font-medium text-primary">Theme</h3>
            <p class="text-xs text-tertiary mt-0.5">Choose your preferred color scheme</p>
          </div>
        </div>

        <!-- Device-only Theme Toggle (self mode only) -->
        <ToggleSwitch
          v-if="!isAdminMode"
          v-model="deviceLocalTheme"
          label="Device-only theme"
          description="Don't sync theme across devices (e.g., use E-Paper theme on your tablet while keeping dark mode on your laptop)"
          @update:modelValue="handleDeviceLocalThemeToggle"
        />

        <!-- System Theme Option -->
        <div>
          <h4 class="text-xs font-medium text-tertiary uppercase tracking-wider mb-2">
            Automatic
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
            Light Themes
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
            Dark Themes
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
            <span class="text-sm text-status-error">Why would you do this to them 😭</span>
          </div>
        </Transition>
      </div>

      <!-- Accessibility Options (self mode only) -->
      <div v-if="!isAdminMode" class="flex flex-col gap-4 pt-2 border-t border-default">
        <div>
          <h3 class="text-sm font-medium text-primary">Accessibility</h3>
          <p class="text-xs text-tertiary mt-0.5">Improve readability and visual distinction</p>
        </div>

        <!-- Color Blind Friendly Mode Toggle -->
        <ToggleSwitch
          v-model="colorBlindMode"
          label="Color blind friendly mode"
          :description="isMonochromaticTheme
            ? 'Always enabled for monochromatic themes like E-Paper and Red Horizon'
            : 'Use distinct shapes for status indicators instead of relying only on colors'"
          :disabled="isMonochromaticTheme"
          @update:modelValue="handleColorBlindModeToggle"
        />
      </div>

      <!-- Display Options (self mode only) -->
      <div v-if="!isAdminMode" class="flex flex-col gap-4 pt-2 border-t border-default">
        <div>
          <h3 class="text-sm font-medium text-primary">Display</h3>
          <p class="text-xs text-tertiary mt-0.5">Adjust layout preferences</p>
        </div>

        <!-- Compact View Toggle -->
        <ToggleSwitch
          v-model="compactView"
          label="Compact view"
          description="Reduce spacing between elements for a denser layout"
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
