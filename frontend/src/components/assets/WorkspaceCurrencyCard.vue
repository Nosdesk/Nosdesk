<script setup lang="ts">
/**
 * Workspace setting: the currency cost fields offer by default. Saves on
 * change. Every amount keeps the currency it was entered in, so a change
 * here never touches costs already recorded.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'

import SearchableDropdown, { type DropdownOption } from '@/components/common/SearchableDropdown.vue'
import { useToastStore } from '@nosdesk/core/stores/toast'
import { useWorkspaceQuery, useWorkspaceQueryCache } from '@/queries/workspaceQuery'
import { extractErrorMessage } from '@/utils/errors'
import { assetSettingsService } from '@nosdesk/core/services/assetSettingsService'
import { currencyName } from '@nosdesk/core/utils/money'

const { $t: t } = useFluent()
const toast = useToastStore()
const queryCache = useWorkspaceQueryCache()
const KEY = ['asset-settings']
const settings = useWorkspaceQuery({ key: KEY, query: () => assetSettingsService.get() })

/** The selected code; '' while the workspace has none. */
const currency = ref('')
const saving = ref(false)

watch(
  () => settings.data.value,
  (s) => {
    if (s) currency.value = s.currency ?? ''
  },
  { immediate: true },
)

const options = computed<DropdownOption[]>(() => [
  { value: '', label: t('asset-currency-not-set') },
  ...(settings.data.value?.currencies ?? []).map((code) => ({
    value: code,
    label: code,
    description: currencyName(code),
  })),
])

const hint = computed(() => {
  if (currency.value) return t('asset-currency-set-hint', { code: currency.value })
  const suggested = settings.data.value?.suggested_currency
  return suggested
    ? t('asset-currency-suggested-hint', { code: suggested })
    : t('asset-currency-unset-hint')
})

async function choose(next: string): Promise<void> {
  if (next === currency.value || saving.value) return
  const previous = currency.value
  currency.value = next
  saving.value = true
  try {
    await assetSettingsService.save({ currency: next || null })
    await queryCache.invalidateQueries({ key: KEY })
    toast.success(t('asset-currency-saved'))
  } catch (e) {
    currency.value = previous
    toast.error(extractErrorMessage(e, t('asset-currency-save-failed')))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <section
    v-if="settings.data.value"
    class="flex flex-col gap-3 bg-surface border border-default rounded-lg p-4"
  >
    <div class="flex flex-col gap-1">
      <h2 class="text-sm font-medium text-primary">{{ t('asset-currency-title') }}</h2>
      <p class="text-xs text-secondary">{{ t('asset-currency-description') }}</p>
    </div>
    <SearchableDropdown
      :model-value="currency"
      :options="options"
      :disabled="saving"
      :aria-label="t('asset-currency-title')"
      :search-placeholder="t('asset-currency-search-placeholder')"
      :empty-message="t('asset-currency-no-matches')"
      size="sm"
      class="self-start w-full sm:w-72"
      @update:model-value="choose"
    />
    <p class="text-xs text-secondary">{{ hint }}</p>
  </section>
</template>
