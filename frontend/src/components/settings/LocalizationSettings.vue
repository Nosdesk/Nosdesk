<script setup lang="ts">
/**
 * Language + timezone preferences for the signed-in user.
 *
 * Locale picker offers the locales bundled in src/i18n plus a
 * "Site default" option (clears the raw pref so the resolver
 * chain returns to site_settings.default_locale). Timezone picker
 * offers `Intl.supportedValuesOf('timeZone')` (~600 IANA names in
 * modern browsers) plus a "Browser-detected" option (sentinel
 * 'system'). Saving PATCHes the user with the two prefs.
 *
 * Optimistic strategy: stamp the new values into dateStore right
 * away so date formatting + active Fluent bundle flip immediately.
 * On API failure revert.
 */
import { computed, ref } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useDateStore } from '@/stores/dateStore'
import { SUPPORTED_LOCALES } from '@/i18n'
import userService from '@/services/userService'
import SectionCard from '@/components/common/SectionCard.vue'
import Spinner from '@/components/common/Spinner.vue'

const authStore = useAuthStore()
const dateStore = useDateStore()

const emit = defineEmits<{
  (e: 'success', message: string): void
  (e: 'error', message: string): void
}>()

const props = defineProps<{
  targetUserUuid?: string
}>()

const isAdminMode = computed(
  () => !!props.targetUserUuid && props.targetUserUuid !== authStore.user?.uuid,
)

const userUuid = computed(
  () => props.targetUserUuid ?? authStore.user?.uuid,
)

// In admin mode we don't have a separate dateStore-per-target; we
// just send the PATCH and trust the user's next /me re-fetch to
// re-sync. So we read raw prefs straight off the auth store when
// editing self, and from authStore.user as a snapshot when admin.
const initialLocale = computed<string>(() => {
  if (isAdminMode.value) {
    return authStore.user?.locale ?? ''
  }
  return dateStore.userLocale ?? ''
})

const initialTimezone = computed<string>(() => {
  if (isAdminMode.value) {
    return authStore.user?.timezone ?? 'system'
  }
  return dateStore.userTimezone
})

const selectedLocale = ref<string>(initialLocale.value)
const selectedTimezone = ref<string>(initialTimezone.value)

// Locale options. Empty-string sentinel = "Site default" → sends
// '' in the PATCH which the backend normalises to null via the
// empty-string-clears semantic in handlers/users.rs.
const localeLabels: Record<string, string> = {
  'en-US': 'English (United States)',
  'en-GB': 'English (United Kingdom)',
  'en-AU': 'English (Australia)',
}
const localeOptions = computed(() => [
  { value: '', label: 'Site default' },
  ...SUPPORTED_LOCALES.map((code) => ({
    value: code,
    label: localeLabels[code] ?? code,
  })),
])

// Timezone options. `Intl.supportedValuesOf('timeZone')` returns
// the IANA tz database — ~600 names. Group with a "Browser-
// detected" sentinel at the top so the default path is one click.
const browserTimezone = computed(() => dateStore.browserTimezone)
const ianaTimezones = computed<string[]>(() => {
  if (typeof Intl !== 'undefined' && 'supportedValuesOf' in Intl) {
    try {
      return (Intl as unknown as { supportedValuesOf: (k: string) => string[] })
        .supportedValuesOf('timeZone')
    } catch {
      return []
    }
  }
  return []
})

const isUpdating = ref(false)

async function save() {
  if (!userUuid.value) return

  const previousLocale = selectedLocale.value
  const previousTimezone = selectedTimezone.value

  isUpdating.value = true

  // Build the PATCH payload. Backend treats the empty string as
  // "clear" and a non-empty string as "set to this".
  const localePayload = selectedLocale.value // '' = clear, otherwise the tag
  // 'system' is the frontend sentinel for "use browser detection";
  // map it to '' so the backend stores null and the resolver
  // chain reflects "no explicit user preference".
  const timezonePayload =
    selectedTimezone.value === 'system' ? '' : selectedTimezone.value

  // Optimistic local update for self. Admin edits skip the local
  // dateStore mutation (the admin's own locale/tz shouldn't shift
  // when they save changes on a different user).
  if (!isAdminMode.value) {
    dateStore.setUserLocale(localePayload === '' ? null : localePayload)
    dateStore.setUserTimezone(
      timezonePayload === '' ? 'system' : timezonePayload,
    )
  }

  try {
    const updated = await userService.updateUser(userUuid.value, {
      locale: localePayload,
      timezone: timezonePayload,
    } as Parameters<typeof userService.updateUser>[1])
    if (!updated) throw new Error('updateUser returned null')

    // For self: re-seed dateStore from the response so the
    // effective_* fields the backend resolved take precedence
    // over our optimistic local guess (e.g. if site default
    // changed under us).
    if (!isAdminMode.value) {
      dateStore.loadFromUser(updated)
    }
    emit('success', 'Language and timezone preferences saved')
  } catch (e) {
    // Revert optimistic state.
    selectedLocale.value = previousLocale
    selectedTimezone.value = previousTimezone
    if (!isAdminMode.value) {
      dateStore.setUserLocale(previousLocale === '' ? null : previousLocale)
      dateStore.setUserTimezone(
        previousTimezone === '' || previousTimezone === 'system'
          ? 'system'
          : previousTimezone,
      )
    }
    emit('error', e instanceof Error ? e.message : 'Failed to save preferences')
  } finally {
    isUpdating.value = false
  }
}

const dirty = computed(
  () =>
    selectedLocale.value !== initialLocale.value ||
    selectedTimezone.value !== initialTimezone.value,
)
</script>

<template>
  <SectionCard content-padding="p-4 sm:p-6">
    <template #leading>
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
          d="M3 5h12M9 3v2m1.048 9.5A18.022 18.022 0 016.412 9m6.088 9h7M11 21l5-10 5 10M12.751 5C11.783 10.77 8.07 15.61 3 18.129"
        />
      </svg>
    </template>
    <template #title>Language &amp; Timezone</template>
    <template #headerActions>
      <Spinner v-if="isUpdating" class="text-accent" />
    </template>

    <div class="flex flex-col gap-5">
      <p class="text-xs text-tertiary">
        Affects message language and how dates render. Site default
        applies when you don't pick one explicitly.
      </p>

      <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <!-- Locale picker -->
        <div class="flex flex-col gap-2">
          <label
            for="locale-select"
            class="text-sm font-medium text-primary"
          >
            Language
          </label>
          <select
            id="locale-select"
            v-model="selectedLocale"
            :disabled="isUpdating"
            class="w-full px-3 py-2 bg-surface-alt text-primary rounded-lg border border-default focus:ring-2 focus:ring-accent focus:outline-none text-sm"
          >
            <option
              v-for="opt in localeOptions"
              :key="opt.value || 'default'"
              :value="opt.value"
            >
              {{ opt.label }}
            </option>
          </select>
        </div>

        <!-- Timezone picker -->
        <div class="flex flex-col gap-2">
          <label
            for="timezone-select"
            class="text-sm font-medium text-primary"
          >
            Timezone
          </label>
          <select
            id="timezone-select"
            v-model="selectedTimezone"
            :disabled="isUpdating"
            class="w-full px-3 py-2 bg-surface-alt text-primary rounded-lg border border-default focus:ring-2 focus:ring-accent focus:outline-none text-sm"
          >
            <option value="system">
              Browser-detected ({{ browserTimezone }})
            </option>
            <option
              v-for="tz in ianaTimezones"
              :key="tz"
              :value="tz"
            >
              {{ tz }}
            </option>
          </select>
        </div>
      </div>

      <div class="flex justify-end">
        <button
          type="button"
          :disabled="!dirty || isUpdating"
          class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 focus:outline-none focus:ring-2 focus:ring-accent transition-colors flex items-center gap-2 whitespace-nowrap disabled:opacity-50 disabled:cursor-not-allowed"
          @click="save"
        >
          <Spinner v-if="isUpdating" />
          {{ isUpdating ? 'Saving...' : 'Save' }}
        </button>
      </div>
    </div>
  </SectionCard>
</template>
