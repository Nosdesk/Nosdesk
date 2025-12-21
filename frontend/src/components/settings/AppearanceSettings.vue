<script setup lang="ts">
import { ref, watch, computed } from 'vue'
import { useThemeStore } from '@/stores/theme'
import { useAuthStore } from '@/stores/auth'
import type { ThemeMode } from '@/themes'
import ThemeCard from '@/components/settings/ThemeCard.vue'
import ToggleSwitch from '@/components/common/ToggleSwitch.vue'

const themeStore = useThemeStore()
const authStore = useAuthStore()

// Props
const props = defineProps<{
  targetUserUuid?: string
}>()

// Emits for notifications
const emit = defineEmits<{
  (e: 'success', message: string): void
  (e: 'error', message: string): void
}>()

// Local reactive state
const selectedTheme = ref<ThemeMode>(themeStore.currentTheme)
const colorBlindMode = ref(themeStore.colorBlindMode)
const compactView = ref(false)
const isUpdating = ref(false)

// Watch theme store changes
watch(
  () => themeStore.currentTheme,
  (newValue) => {
    selectedTheme.value = newValue
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

  // Update local theme
  themeStore.setTheme(themeId)

  // Sync to backend
  if (userUuid.value) {
    const success = await themeStore.syncThemeToBackend(userUuid.value)

    if (success) {
      const themeName = themeId === 'system' ? 'System' : themeStore.effectiveTheme.meta.name
      emit('success', `Theme changed to ${themeName}`)
    } else {
      emit('error', 'Failed to save theme preference')
      // Revert to previous theme on failure
      selectedTheme.value = previousTheme
      themeStore.setTheme(previousTheme)
    }
  }

  isUpdating.value = false
}

// Handle color blind mode toggle
const handleColorBlindModeToggle = () => {
  themeStore.setColorBlindMode(colorBlindMode.value)
  emit('success', `Color blind friendly mode ${colorBlindMode.value ? 'enabled' : 'disabled'}`)
}

// Handle compact view toggle
const handleCompactViewToggle = () => {
  emit('success', `Compact view ${compactView.value ? 'enabled' : 'disabled'}`)
}
</script>

<template>
  <div
    class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden"
  >
    <!-- Header -->
    <div class="px-4 sm:px-6 py-4 bg-surface-alt border-b border-default">
      <div class="flex items-center justify-between gap-3">
        <div class="flex items-center gap-3">
          <div
            class="w-8 h-8 bg-purple-500/20 rounded-lg flex items-center justify-center flex-shrink-0"
          >
            <svg
              xmlns="http://www.w3.org/2000/svg"
              class="h-4 w-4 text-purple-500"
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
          </div>
          <div>
            <h2 class="text-base sm:text-lg font-semibold text-primary">Appearance</h2>
            <p class="text-xs text-secondary hidden sm:block">Customize how Nosdesk looks</p>
          </div>
        </div>
        <div
          v-if="isUpdating || themeStore.isSyncing"
          class="flex items-center gap-2 text-accent"
        >
          <div
            class="animate-spin h-4 w-4 border-2 border-accent border-t-transparent rounded-full"
          ></div>
        </div>
      </div>
    </div>

    <div class="p-4 sm:p-6 flex flex-col gap-6">
      <!-- Theme Selection -->
      <div class="flex flex-col gap-4">
        <div class="flex items-center justify-between">
          <div>
            <h3 class="text-sm font-medium text-primary">Theme</h3>
            <p class="text-xs text-tertiary mt-0.5">Choose your preferred color scheme</p>
          </div>
        </div>

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
      </div>

      <!-- Accessibility Options -->
      <div class="flex flex-col gap-4 pt-2 border-t border-default">
        <div>
          <h3 class="text-sm font-medium text-primary">Accessibility</h3>
          <p class="text-xs text-tertiary mt-0.5">Improve readability and visual distinction</p>
        </div>

        <!-- Color Blind Friendly Mode Toggle -->
        <ToggleSwitch
          v-model="colorBlindMode"
          label="Color blind friendly mode"
          description="Use distinct shapes for status indicators instead of relying only on colors"
          @update:modelValue="handleColorBlindModeToggle"
        />
      </div>

      <!-- Display Options -->
      <div class="flex flex-col gap-4 pt-2 border-t border-default">
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
  </div>
</template>
