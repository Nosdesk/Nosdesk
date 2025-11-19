/**
 * Date Store - User Timezone Preferences
 *
 * Manages user timezone preferences and automatically updates global date configuration.
 * Ready for backend integration to persist user preferences.
 */

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { setDateConfig, getUserTimezone, type DateConfig } from '@/utils/dateUtils'

export const useDateStore = defineStore('date', () => {
  // User's preferred timezone (stored in user preferences)
  // 'system' means use browser's detected timezone
  const userTimezone = ref<string>('system')

  // Detected browser timezone
  const browserTimezone = computed(() => getUserTimezone())

  // Effective timezone (what we actually use)
  const effectiveTimezone = computed(() => {
    if (userTimezone.value === 'system') {
      return browserTimezone.value
    }
    return userTimezone.value
  })

  // Locale preference
  const locale = ref<string>('en-US')

  // Update global date config whenever preferences change
  function updateGlobalConfig() {
    setDateConfig({
      defaultTimezone: effectiveTimezone.value,
      defaultLocale: locale.value
    })
  }

  // Set user timezone preference
  function setUserTimezone(timezone: string) {
    userTimezone.value = timezone
    updateGlobalConfig()
    // TODO: Persist to backend user preferences
    // await userService.updatePreferences({ timezone })
  }

  // Auto-detect and set browser timezone
  function autoDetectTimezone() {
    userTimezone.value = 'system'
    updateGlobalConfig()
  }

  // Load timezone from user profile (called on app init)
  function loadTimezoneFromUser(user: { timezone?: string }) {
    if (user.timezone) {
      userTimezone.value = user.timezone
      updateGlobalConfig()
    }
  }

  // Initialize config on store creation
  updateGlobalConfig()

  return {
    userTimezone,
    browserTimezone,
    effectiveTimezone,
    locale,
    setUserTimezone,
    autoDetectTimezone,
    loadTimezoneFromUser
  }
})
