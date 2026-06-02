<script setup lang="ts">
/**
 * Header time-range chip cluster
 * (docs/dashboard-and-analytics-plan.md decision 5).
 *
 * Six preset chips + a Custom popover. URL-bound via useTimeRange,
 * so a Slack-shared link reproduces the same view. The active chip
 * is highlighted; Custom opens a small from/to picker that calls
 * `setCustomRange`.
 *
 * Keyboard shortcut `T` focuses this cluster (registered by
 * useDashboardKeybindings in a later wave); the rendered chips are
 * normal buttons so Tab traversal already works without it.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { useTimeRange, type TimeRangePreset } from '@/composables/useTimeRange'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const { preset, customFrom, customTo, setPreset, setCustomRange } = useTimeRange()

const PRESETS: { id: TimeRangePreset; key: string }[] = [
  { id: 'today', key: 'dashboard-time-range-today' },
  { id: '7d', key: 'dashboard-time-range-7d' },
  { id: '30d', key: 'dashboard-time-range-30d' },
  { id: '90d', key: 'dashboard-time-range-90d' },
  { id: 'quarter', key: 'dashboard-time-range-quarter' },
  { id: 'custom', key: 'dashboard-time-range-custom' },
]

const showCustomPopover = ref(false)
const customFromInput = ref<string>('')
const customToInput = ref<string>('')

function isActive(id: TimeRangePreset): boolean {
  return preset.value === id
}

function pickPreset(id: TimeRangePreset): void {
  if (id === 'custom') {
    customFromInput.value = customFrom.value ?? ''
    customToInput.value = customTo.value ?? ''
    showCustomPopover.value = true
    return
  }
  showCustomPopover.value = false
  setPreset(id)
}

function applyCustom(): void {
  if (!customFromInput.value || !customToInput.value) return
  setCustomRange(customFromInput.value, customToInput.value)
  showCustomPopover.value = false
}

const customLabel = computed(() => {
  if (preset.value !== 'custom') return t('dashboard-time-range-custom')
  if (customFrom.value && customTo.value) {
    return `${customFrom.value} → ${customTo.value}`
  }
  return t('dashboard-time-range-custom')
})
</script>

<template>
  <div class="relative inline-flex items-center gap-0.5 rounded-md border border-default bg-surface px-0.5 py-0.5 text-xs">
    <button
      v-for="p in PRESETS"
      :key="p.id"
      type="button"
      :class="[
        'rounded px-2 py-1 transition-colors',
        isActive(p.id)
          ? 'bg-accent text-on-accent'
          : 'text-secondary hover:bg-surface-hover hover:text-primary',
      ]"
      @click="pickPreset(p.id)"
    >
      {{ p.id === 'custom' ? customLabel : t(p.key) }}
    </button>

    <div
      v-if="showCustomPopover"
      class="absolute right-0 top-[calc(100%+4px)] z-10 flex flex-col gap-2 rounded-md border border-default bg-surface p-3 shadow-lg"
    >
      <label class="flex flex-col gap-1 text-xs text-secondary">
        {{ t('dashboard-time-range-custom-from') }}
        <input
          v-model="customFromInput"
          type="datetime-local"
          class="rounded border border-default bg-surface px-2 py-1 text-xs text-primary"
        />
      </label>
      <label class="flex flex-col gap-1 text-xs text-secondary">
        {{ t('dashboard-time-range-custom-to') }}
        <input
          v-model="customToInput"
          type="datetime-local"
          class="rounded border border-default bg-surface px-2 py-1 text-xs text-primary"
        />
      </label>
      <div class="flex gap-2 justify-end">
        <button
          type="button"
          class="rounded px-2 py-1 text-xs text-secondary hover:bg-surface-hover"
          @click="showCustomPopover = false"
        >
          {{ t('dashboard-time-range-custom-cancel') }}
        </button>
        <button
          type="button"
          class="rounded bg-accent px-2 py-1 text-xs text-on-accent hover:opacity-90 disabled:opacity-50"
          :disabled="!customFromInput || !customToInput"
          @click="applyCustom"
        >
          {{ t('dashboard-time-range-custom-apply') }}
        </button>
      </div>
    </div>
  </div>
</template>
