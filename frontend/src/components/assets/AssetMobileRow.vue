<script setup lang="ts">
/**
 * One asset as a tap-to-open card in the mobile list. Extracted from
 * AssetsListView so the flat inventory list and the planning-lens
 * drill-down render rows identically.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'
import AssetStatusBadge from '@/components/assets/AssetStatusBadge.vue'
import { useAssetKindsQuery } from '@/composables/useAssetKindsQuery'
import type { Asset } from '@/types/asset'

const props = defineProps<{ asset: Asset }>()
const emit = defineEmits<{ (e: 'open', asset: Asset): void }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const { kinds } = useAssetKindsQuery()
const kindLabel = computed(() => {
  const match = kinds.value.find((k) => k.slug === props.asset.kind)
  return match?.label ?? props.asset.kind
})

const isLowStock = computed(() => {
  const q = props.asset.quantity
  const th = props.asset.low_stock_threshold
  if (q == null || th == null) return false
  return parseFloat(q) <= parseFloat(th)
})
</script>

<template>
  <div
    class="flex items-center gap-3 px-3 py-2.5 hover:bg-surface-hover active:bg-surface-alt transition-colors cursor-pointer border-t border-default first:border-t-0"
    @click="emit('open', asset)"
  >
    <div class="w-10 h-10 rounded-lg bg-surface-alt flex items-center justify-center flex-shrink-0">
      <Icon name="device" size="md" class="text-secondary" />
    </div>

    <div class="flex-1 min-w-0">
      <div class="flex items-center gap-1.5">
        <span class="text-sm text-primary font-medium truncate">{{ asset.name }}</span>
        <div v-if="asset.groups?.length" class="flex items-center gap-1 flex-shrink-0">
          <span
            v-for="group in asset.groups.slice(0, 3)"
            :key="group.id"
            class="w-1.5 h-1.5 rounded-full"
            :style="{ backgroundColor: group.color || 'var(--color-text-tertiary)' }"
            :title="group.name"
          />
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5 mt-1 text-xs">
        <span v-if="asset.model" class="text-secondary">{{ asset.model }}</span>
        <span v-if="asset.location" class="text-tertiary truncate max-w-[140px]">{{ asset.location }}</span>
        <span class="text-tertiary">{{ kindLabel }}</span>
        <span v-if="asset.serial_number" class="text-tertiary font-mono">{{ asset.serial_number }}</span>
      </div>

      <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5 mt-0.5 text-xs">
        <span v-if="asset.attributes?.hostname" class="text-tertiary font-mono truncate max-w-[160px]">{{ asset.attributes.hostname }}</span>
        <span v-if="asset.primary_user" class="text-secondary truncate max-w-[120px]">{{ asset.primary_user.name }}</span>
        <span
          v-if="isLowStock"
          class="inline-flex items-center px-1.5 py-0.5 rounded font-medium border bg-status-warning-muted text-status-warning border-status-warning/30"
        >
          {{ t('assets-list-low-stock-badge') }}
        </span>
        <AssetStatusBadge :status="asset.status || 'in_service'" />
        <span
          v-if="asset.attributes?.warranty_status"
          class="inline-flex items-center px-1.5 py-0.5 rounded font-medium border"
          :class="{
            'bg-status-success-muted text-status-success border-status-success/30': asset.attributes.warranty_status === 'Active',
            'bg-status-warning-muted text-status-warning border-status-warning/30': asset.attributes.warranty_status === 'Warning',
            'bg-status-error-muted text-status-error border-status-error/30': asset.attributes.warranty_status === 'Expired',
            'bg-surface-alt text-secondary border-default': asset.attributes.warranty_status === 'Unknown'
          }"
        >
          {{ asset.attributes.warranty_status }}
        </span>
      </div>
    </div>

    <Icon name="chevronRight" size="sm" class="text-tertiary flex-shrink-0" />
  </div>
</template>
