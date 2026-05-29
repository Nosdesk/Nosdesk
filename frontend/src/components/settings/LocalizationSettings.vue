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
import { useFluent } from 'fluent-vue'
import { useAuthStore } from '@/stores/auth'
import { useDateStore } from '@/stores/dateStore'
import { SUPPORTED_LOCALES } from '@/i18n'
import userService from '@/services/userService'
import SectionCard from '@/components/common/SectionCard.vue'
import Spinner from '@/components/common/Spinner.vue'
import Button from '@/components/common/Button.vue'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import SearchableDropdown, { type DropdownOption } from '@/components/common/SearchableDropdown.vue'
import { useTimezoneOptions } from '@/composables/useTimezoneOptions'

const authStore = useAuthStore()
const dateStore = useDateStore()
// `$t` from fluent-vue. Templates can call `$t('key')` directly
// via the registered global property; script-side strings (toast
// messages) go through `format()`.
const fluent = useFluent()

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
// empty-string-clears semantic in handlers/users.rs. Labels go
// through Fluent so the picker itself reflects the active locale
// (recomputed when `dateStore.locale` flips because `$t` reads
// from the reactive bundle).
const localeOptions = computed(() => [
  { value: '', label: fluent.$t('settings-locale-site-default') },
  ...SUPPORTED_LOCALES.map((code) => ({
    value: code,
    label: fluent.$t(`settings-locale-${code}`) || code,
  })),
])

// Timezone options come from the shared composable so every
// IANA-picker in the app reads the same way. We prepend a
// "Use device timezone" sentinel here because that meaning only
// applies to per-user preferences, not to workspace-pinned
// calendars.
const browserTimezone = computed(() => dateStore.browserTimezone)
const ianaOptions = useTimezoneOptions()

const timezoneOptions = computed<DropdownOption[]>(() => {
  const opts: DropdownOption[] = []
  opts.push({
    value: 'system',
    label: fluent.$t('settings-timezone-use-device'),
    description: browserTimezone.value,
  })
  return [...opts, ...ianaOptions.value]
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
    })
    if (!updated) throw new Error('updateUser returned null')

    // For self: re-seed dateStore from the response so the
    // effective_* fields the backend resolved take precedence
    // over our optimistic local guess (e.g. if site default
    // changed under us).
    if (!isAdminMode.value) {
      dateStore.loadFromUser(updated)
    }
    emit('success', fluent.$t('settings-localization-saved'))
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
    emit(
      'error',
      e instanceof Error ? e.message : fluent.$t('settings-localization-save-failed'),
    )
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
    <template #title>{{ $t('settings-localization-title') }}</template>
    <template #headerActions>
      <Spinner v-if="isUpdating" class="text-accent" />
    </template>

    <div class="flex flex-col gap-5">
      <p class="text-xs text-tertiary">
        {{ $t('settings-localization-help') }}
      </p>

      <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <!-- Locale picker -->
        <div class="flex flex-col gap-2">
          <label class="text-sm font-medium text-primary">
            {{ $t('settings-language-label') }}
          </label>
          <BaseDropdown
            v-model="selectedLocale"
            :options="localeOptions"
            :disabled="isUpdating"
            :placeholder="$t('settings-language-label')"
            size="sm"
          />
        </div>

        <!-- Timezone picker -->
        <div class="flex flex-col gap-2">
          <label class="text-sm font-medium text-primary">
            {{ $t('settings-timezone-label') }}
          </label>
          <SearchableDropdown
            v-model="selectedTimezone"
            :options="timezoneOptions"
            :disabled="isUpdating"
            :placeholder="$t('settings-timezone-label')"
            :search-placeholder="$t('settings-timezone-search-placeholder')"
            :empty-message="$t('settings-timezone-no-matches')"
            size="sm"
          />
        </div>
      </div>

      <div class="flex justify-end">
        <Button type="button" :disabled="!dirty" :loading="isUpdating" @click="save">
          {{ $t('settings-save') }}
        </Button>
      </div>
    </div>
  </SectionCard>
</template>
