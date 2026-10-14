<script setup lang="ts">
/**
 * The inventory list's Last status change cell: a short date, then the
 * newest transition, on one line. The date leads so a narrow column
 * truncates the transition, not the date. The title gives the full date and, for staff, who
 * made the change and why.
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
  <div v-if="cell" class="flex items-center gap-1.5 min-w-0 text-xs" :title="cell.title">
    <span class="text-tertiary tabular-nums whitespace-nowrap flex-shrink-0">{{ cell.date }}</span>
    <span class="text-secondary truncate">{{ cell.label }}</span>
  </div>
</template>
