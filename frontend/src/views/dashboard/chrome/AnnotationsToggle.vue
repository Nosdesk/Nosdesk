<script setup lang="ts">
/**
 * Audit-log annotation overlay toggle
 * (docs/dashboard-and-analytics-plan.md decision 7).
 *
 * Wave 1 ships the toggle UI + URL-state plumbing only; Wave 7
 * wires the actual marker rendering on time-series widgets and
 * the kind-picker popover. The toggle defaults to off so the
 * dashboard stays clean for the common case (Grafana's
 * "annotations clutter" lesson).
 *
 * URL state: `?annotations=on` enables; absent or any other value
 * disables. Phase 8's kind picker adds `?annotation_kinds=rules,sla`
 * (default: all kinds when annotations=on).
 */
import { computed } from 'vue'
import { useRoute, useRouter, type LocationQuery } from 'vue-router'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

const route = useRoute()
const router = useRouter()

const enabled = computed<boolean>(() => route.query.annotations === 'on')

function toggle(): void {
  const next: LocationQuery = { ...route.query }
  if (enabled.value) {
    delete next.annotations
    delete next.annotation_kinds
  } else {
    next.annotations = 'on'
  }
  router.replace({ query: next })
}
</script>

<template>
  <button
    type="button"
    :class="[
      'inline-flex items-center gap-1.5 rounded-md border px-2 py-1 text-xs transition-colors',
      enabled
        ? 'border-accent bg-accent/10 text-accent'
        : 'border-default bg-surface text-secondary hover:bg-surface-hover hover:text-primary',
    ]"
    :aria-pressed="enabled"
    :title="t('dashboard-annotations-toggle-tooltip')"
    @click="toggle"
  >
    <Icon name="info" class="w-3.5 h-3.5" />
    <span>{{ t('dashboard-annotations-toggle-label') }}</span>
  </button>
</template>
