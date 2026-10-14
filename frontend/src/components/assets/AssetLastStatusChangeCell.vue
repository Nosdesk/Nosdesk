<script setup lang="ts">
/**
 * The inventory list's Status changed cell: a short date and the status the
 * asset moved from ("Oct 14 · from In service"). The Status column shows
 * where it went. The title has the whole transition, the full date and, for
 * staff, who made the change and why.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import type { AssetLatestLifecycle } from '@nosdesk/core/types/asset'
import { lastStatusChangeCell } from '@/utils/assetLoanStatusColumns'

const props = defineProps<{ event: AssetLatestLifecycle | null | undefined }>()

const fluent = useFluent()
const cell = computed(() => lastStatusChangeCell((key, args) => fluent.$t(key, args), props.event))
</script>

<template>
  <div v-if="cell" class="flex items-center gap-1 min-w-0 text-xs" :title="cell.title">
    <span class="text-secondary tabular-nums whitespace-nowrap flex-shrink-0">{{ cell.date }}</span>
    <span class="text-tertiary flex-shrink-0" aria-hidden="true">·</span>
    <span class="text-tertiary truncate">{{ cell.from }}</span>
  </div>
</template>
