// src/stores/theme.ts
import { logger } from '@/utils/logger';
import { defineStore } from 'pinia'
import { ref, computed, watch } from 'vue'
import userService from '@/services/userService'
import type { User } from '@/services/userService'

export type Theme = 'system' | 'light' | 'dark'

export const useThemeStore = defineStore('theme', () => {
  // Use localStorage to persist theme preference, default to 'system'
  const savedTheme = localStorage.getItem('theme') as Theme | null
  const currentTheme = ref<Theme>(savedTheme || 'system')
  const isSyncing = ref<boolean>(false)

  // Computed property to get the effective theme (resolves 'system' to 'light' or 'dark')
  const effectiveTheme = computed<'light' | 'dark'>(() => {
    if (currentTheme.value === 'system') {
      return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
    }
    return currentTheme.value
  })

  const isDarkMode = computed(() => effectiveTheme.value === 'dark')

  // Update the DOM to reflect the current theme
  // Uses Tailwind's class-based dark mode convention
  function updateTheme(): void {
    if (effectiveTheme.value === 'dark') {
      document.documentElement.classList.add('dark')
    } else {
      document.documentElement.classList.remove('dark')
    }
  }

  // Set theme directly (accepts 'system', 'light', or 'dark')
  function setTheme(theme: Theme): void {
    if (theme !== 'system' && theme !== 'light' && theme !== 'dark') {
      logger.warn('Invalid theme:', theme, '- must be system, light, or dark')
      return
    }
    currentTheme.value = theme
    localStorage.setItem('theme', theme)
    updateTheme()
  }

  // Toggle between light and dark (skips 'system')
  function toggleTheme(): void {
    if (currentTheme.value === 'system') {
      // If currently on system, toggle to the opposite of current system preference
      setTheme(isDarkMode.value ? 'light' : 'dark')
    } else {
      setTheme(currentTheme.value === 'light' ? 'dark' : 'light')
    }
  }

  // Sync theme to backend
  async function syncThemeToBackend(userUuid: string): Promise<boolean> {
    if (!userUuid) {
      logger.warn('Cannot sync theme: no user UUID provided')
      return false
    }

    try {
      isSyncing.value = true

      await userService.updateUser(userUuid, {
        theme: currentTheme.value
      })

      logger.debug('✅ Theme synced to backend:', currentTheme.value)
      return true
    } catch (error) {
      logger.error('Failed to sync theme to backend:', error)
      return false
    } finally {
      isSyncing.value = false
    }
  }

  // Load theme from user profile
  function loadThemeFromUser(user: User | null): void {
    if (user && user.theme) {
      logger.debug('📥 Loading theme from user profile:', user.theme)
      setTheme(user.theme as Theme)
    }
  }

  // Initialize theme on store creation
  updateTheme()

  // Watch for changes in effectiveTheme and update DOM
  watch(effectiveTheme, () => {
    updateTheme()
  })

  // Listen for system theme changes (only affects 'system' theme)
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
    if (currentTheme.value === 'system') {
      updateTheme()
    }
  })

  return {
    currentTheme,
    effectiveTheme,
    isDarkMode,
    isSyncing,
    setTheme,
    toggleTheme,
    syncThemeToBackend,
    loadThemeFromUser,
  }
})
