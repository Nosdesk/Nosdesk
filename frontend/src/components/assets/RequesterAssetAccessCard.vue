<script setup lang="ts">
/**
 * Workspace setting: what requesters see of the workspace's assets, all or
 * none. Saves on change. Staff always see every asset.
 */
import { computed, ref, watch } from 'vue'
import { useQuery, useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import SegmentedControl from '@/components/common/SegmentedControl.vue'
import { useToastStore } from '@nosdesk/core/stores/toast'
import { extractErrorMessage } from '@/utils/errors'
import {
  assetSettingsService,
  type RequesterAssetVisibility,
} from '@nosdesk/core/services/assetSettingsService'

const { $t: t } = useFluent()
const toast = useToastStore()
const queryCache = useQueryCache()
const KEY = ['asset-settings']
const settings = useQuery({ key: KEY, query: () => assetSettingsService.get() })

const visibility = ref<RequesterAssetVisibility>('all')
const saving = ref(false)

watch(
  () => settings.data.value,
  (s) => {
    if (s) visibility.value = s.requester_visibility
  },
  { immediate: true },
)

const options = computed(() => [
  { value: 'all' as const, label: t('asset-access-all') },
  { value: 'none' as const, label: t('asset-access-none') },
])

async function choose(next: RequesterAssetVisibility): Promise<void> {
  if (next === visibility.value || saving.value) return
  const previous = visibility.value
  visibility.value = next
  saving.value = true
  try {
    await assetSettingsService.save({ requester_visibility: next })
    await queryCache.invalidateQueries({ key: KEY })
    toast.success(t('asset-access-saved'))
  } catch (e) {
    visibility.value = previous
    toast.error(extractErrorMessage(e, t('asset-access-save-failed')))
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
      <h2 class="text-sm font-medium text-primary">{{ t('asset-access-title') }}</h2>
      <p class="text-xs text-secondary">{{ t('asset-access-description') }}</p>
    </div>
    <SegmentedControl
      :model-value="visibility"
      :options="options"
      :aria-label="t('asset-access-title')"
      class="self-start"
      @update:model-value="choose"
    />
    <p class="text-xs text-secondary">
      {{ visibility === 'none' ? t('asset-access-none-hint') : t('asset-access-all-hint') }}
    </p>
  </section>
</template>
