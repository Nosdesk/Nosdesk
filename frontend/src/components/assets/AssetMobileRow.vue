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
import { StatusBadgeCell } from '@/components/common/cells'
import type { Asset } from '@nosdesk/core/types/asset'

const props = defineProps<{ asset: Asset }>()
const emit = defineEmits<{ (e: 'open', asset: Asset): void }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const isLowStock = computed(() => {
  const q = props.asset.quantity
  const th = props.asset.low_stock_threshold
  if (q == null || th == null) return false
  return parseFloat(q) <= parseFloat(th)
})

const warrantyStatus = computed(
  () => (props.asset.attributes?.warranty_status as string | undefined) ?? '',
)
/** Expiring or already expired: the only two worth a badge, matching
 *  the desktop table. Everything else stays quiet. */
const needsWarrantyAttention = computed(
  () => warrantyStatus.value === 'Warning' || warrantyStatus.value === 'Expired',
)
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
        <!-- Kind is dropped for the same reason the desktop table
             hides it by default: it reads the same for every row on a
             device-only workspace. The low-stock badge below still
             marks the one kind that behaves differently. -->
        <span v-if="asset.model" class="text-secondary">{{ asset.model }}</span>
        <span v-if="asset.location" class="text-tertiary truncate max-w-[140px]">{{ asset.location }}</span>
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
        <AssetStatusBadge :status="asset.status || 'in_service'" variant="plain" />
        <!-- Shared cell rather than the colour map this used to
             hand-roll, so the warranty palette lives in one place. -->
        <StatusBadgeCell
          v-if="needsWarrantyAttention"
          type="warranty"
          size="xs"
          :value="warrantyStatus"
        />
      </div>
    </div>

    <Icon name="chevronRight" size="sm" class="text-tertiary flex-shrink-0" />
  </div>
</template>
