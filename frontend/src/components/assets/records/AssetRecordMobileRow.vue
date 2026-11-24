<script setup lang="ts">
/**
 * One record in the list below 1024px, on two lines that share the
 * table's two edges: what was done on the left, what it cost on the right.
 * Line two is the same facts the table's columns carry, in the same order:
 * date, asset, type, vendor.
 */
import type { AssetRecordListRow } from '@nosdesk/core/types/asset'

defineProps<{
  record: AssetRecordListRow
  title: string
  day: string
  /** Empty when the list is grouped by it, as for `assetTag` and `vendor`. */
  typeLabel: string
  assetTag: string
  vendor: string | null
  cost: string | null
  inProgress: string | null
}>()
const emit = defineEmits<{ (e: 'open', record: AssetRecordListRow): void }>()
</script>

<template>
  <button
    type="button"
    class="w-full flex flex-col gap-0.5 px-4 py-2.5 text-left border-t border-default hover:bg-surface-hover active:bg-surface-alt transition-colors"
    data-record-row
    @click="emit('open', record)"
  >
    <span class="flex items-baseline gap-3">
      <span class="min-w-0 flex-1 truncate text-sm font-medium text-primary">{{ title }}</span>
      <span v-if="cost" class="shrink-0 text-sm tabular-nums text-primary">{{ cost }}</span>
    </span>
    <span class="flex items-baseline gap-2 text-xs text-tertiary">
      <span class="shrink-0 tabular-nums">{{ day }}</span>
      <span v-if="assetTag" class="shrink-0 font-mono text-secondary">{{ assetTag }}</span>
      <span v-if="typeLabel" class="min-w-0 shrink truncate">{{ typeLabel }}</span>
      <span v-if="vendor" class="min-w-0 flex-1 basis-0 truncate">{{ vendor }}</span>
      <span v-if="inProgress" class="ml-auto shrink-0 font-medium text-primary">{{ inProgress }}</span>
    </span>
  </button>
</template>
