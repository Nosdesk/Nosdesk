<script setup lang="ts">
/**
 * Compare-to-prior global toggle
 * (docs/dashboard-and-analytics-plan.md decision 6).
 *
 * When on, every time-series renders a faint overlay of the prior
 * window and every KPI shows a delta vs prior. Off by default per
 * the synthesis: comparison data is meaningful but noisy enough
 * that users should opt in.
 *
 * The toggle is URL-bound via useTimeRange so a shared link
 * reproduces the state.
 */
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'
import { useTimeRange } from '@/composables/useTimeRange'

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

const { compare, setCompare } = useTimeRange()
</script>

<template>
  <button
    type="button"
    :class="[
      'inline-flex items-center gap-1.5 rounded-md border px-2 py-1 text-xs transition-colors',
      compare
        ? 'border-accent bg-accent/10 text-accent'
        : 'border-default bg-surface text-secondary hover:bg-surface-hover hover:text-primary',
    ]"
    :aria-pressed="compare"
    :title="t('dashboard-compare-toggle-tooltip')"
    @click="setCompare(!compare)"
  >
    <Icon name="history" class="w-3.5 h-3.5" />
    <span>{{ t('dashboard-compare-toggle-label') }}</span>
  </button>
</template>
