// src/stores/theme.js
import { defineStore } from 'pinia'
import { ref, computed, watch } from 'vue'
import userService from '@/services/userService'

export const useThemeStore = defineStore('theme', () => {
  // Use localStorage to persist theme preference, default to 'system'
  const savedTheme = localStorage.getItem('theme')
  const currentTheme = ref(savedTheme || 'system')
  const isSyncing = ref(false)

  // Computed property to get the effective theme (resolves 'system' to 'light' or 'dark')
  const effectiveTheme = computed(() => {
    if (currentTheme.value === 'system') {
      return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
    }
    return currentTheme.value
  })

  const isDarkMode = computed(() => effectiveTheme.value === 'dark')

  // Update the DOM to reflect the current theme
  // Uses Tailwind's class-based dark mode convention
  function updateTheme() {
    if (effectiveTheme.value === 'dark') {
      document.documentElement.classList.add('dark')
    } else {
      document.documentElement.classList.remove('dark')
    }
  }

  // Set theme directly (accepts 'system', 'light', or 'dark')
  function setTheme(theme) {
    if (theme !== 'system' && theme !== 'light' && theme !== 'dark') {
      console.warn('Invalid theme:', theme, '- must be system, light, or dark')
      return
    }
    currentTheme.value = theme
    localStorage.setItem('theme', theme)
    updateTheme()
  }

  // Toggle between light and dark (skips 'system')
  function toggleTheme() {
    if (currentTheme.value === 'system') {
      // If currently on system, toggle to the opposite of current system preference
      setTheme(isDarkMode.value ? 'light' : 'dark')
    } else {
      setTheme(currentTheme.value === 'light' ? 'dark' : 'light')
    }
  }

  // Sync theme to backend
  async function syncThemeToBackend(userUuid) {
    if (!userUuid) {
      console.warn('Cannot sync theme: no user UUID provided')
      return false
    }

    try {
      isSyncing.value = true

      await userService.updateUser(userUuid, {
        theme: currentTheme.value
      })

      console.log('✅ Theme synced to backend:', currentTheme.value)
      return true
    } catch (error) {
      console.error('Failed to sync theme to backend:', error)
      return false
    } finally {
      isSyncing.value = false
    }
  }

  // Load theme from user profile
  function loadThemeFromUser(user) {
    if (user && user.theme) {
      console.log('📥 Loading theme from user profile:', user.theme)
      setTheme(user.theme)
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
