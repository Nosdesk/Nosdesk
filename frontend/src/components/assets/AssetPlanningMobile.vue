<script setup lang="ts">
/**
 * Mobile rendering of a fleet-planning lens. The desktop DataTable's
 * grouped rows don't translate to a phone, so the same bucket model is
 * shown as a glanceable summary: a grid of bucket cards (count is the
 * hero), tap one to drill into its devices, and roll out the whole group
 * with one action. Whole-bucket rollout sidesteps mobile multi-select.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'
import Button from '@/components/common/Button.vue'
import AssetMobileRow from '@/components/assets/AssetMobileRow.vue'
import type { GroupBucket } from '@/composables/useListGrouping'
import type { Asset } from '@nosdesk/core/types/asset'

const props = defineProps<{
  buckets: GroupBucket<Asset>[]
  /** Display name of the active group axis (toolbar context). */
  axisLabel: string
}>()

const emit = defineEmits<{
  (e: 'open', asset: Asset): void
  (e: 'rollout', bucket: GroupBucket<Asset>): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const selectedKey = ref<string | null>(null)
const selectedBucket = computed(() =>
  props.buckets.find((b) => b.key === selectedKey.value) ?? null,
)

// Drop back to the summary if the selected bucket disappears (filter or
// lens change rebuilds the buckets).
watch(
  () => props.buckets,
  () => {
    if (selectedKey.value && !props.buckets.some((b) => b.key === selectedKey.value)) {
      selectedKey.value = null
    }
  },
)

// Warranty-window buckets carry severity; colour the count accordingly so
// the at-risk groups read at a glance. Other axes stay neutral.
function accentClass(key: string): string {
  const map: Record<string, string> = {
    'ww:expired': 'text-status-error',
    'ww:expiring_30d': 'text-status-error',
    'ww:expiring_90d': 'text-status-warning',
    'ww:active': 'text-status-success',
    'ww:unknown': 'text-tertiary',
  }
  return map[key] ?? 'text-primary'
}
</script>

<template>
  <!-- Summary grid -->
  <div v-if="!selectedBucket" class="p-4">
    <p class="text-xs text-tertiary mb-3">
      {{ t('asset-planning-mobile-hint', { axis: axisLabel }) }}
    </p>
    <div class="grid grid-cols-2 gap-3">
      <button
        v-for="b in buckets"
        :key="b.key"
        type="button"
        class="flex flex-col items-start gap-1 rounded-lg border border-default bg-surface p-3 text-left transition-colors hover:border-strong active:bg-surface-alt"
        @click="selectedKey = b.key"
      >
        <span class="text-2xl font-semibold tabular-nums leading-none" :class="accentClass(b.key)">
          {{ b.items.length }}
        </span>
        <span class="text-xs text-secondary line-clamp-2">{{ b.label }}</span>
      </button>
    </div>
  </div>

  <!-- Drill-in: one bucket's devices + whole-bucket rollout -->
  <div v-else class="flex flex-col">
    <div class="sticky top-0 z-10 flex items-center gap-2 border-b border-subtle bg-surface px-2 py-2.5">
      <button
        type="button"
        class="flex items-center gap-1 rounded-md px-2 py-1 text-sm text-secondary hover:bg-surface-hover"
        @click="selectedKey = null"
      >
        <Icon name="chevronLeft" size="sm" />
        {{ t('asset-planning-mobile-back') }}
      </button>
      <span class="text-sm font-medium text-primary truncate">{{ selectedBucket.label }}</span>
      <span class="ml-auto text-xs tabular-nums text-tertiary">{{ selectedBucket.items.length }}</span>
    </div>

    <AssetMobileRow
      v-for="asset in selectedBucket.items"
      :key="asset.id"
      :asset="asset"
      @open="emit('open', $event)"
    />

    <!-- Whole-bucket rollout. Sticky so it stays reachable while the
         device list scrolls. -->
    <div class="sticky bottom-0 border-t border-subtle bg-surface/95 backdrop-blur p-3">
      <Button variant="primary" block icon="send" @click="emit('rollout', selectedBucket)">
        {{ t('asset-rollout-create-action', { count: selectedBucket.items.length }) }}
      </Button>
    </div>
  </div>
</template>
