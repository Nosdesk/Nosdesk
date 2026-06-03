<script setup lang="ts">
/**
 * Header time-range picker (docs/dashboard-and-analytics-plan.md
 * decision 5; v1 design language collapses the original 6-chip
 * cluster into a single trigger + popover).
 *
 * The trigger button shows the active range; clicking opens a
 * popover containing the five preset entries plus a custom-range
 * date input. State plumbing (useTimeRange) is unchanged so
 * KpiTile / LineChart / HorizontalBar consumers keep working
 * without modification.
 *
 * Keyboard: Esc closes the popover; click-outside also closes.
 * Filename retained as TimeRangeChipCluster.vue so import sites
 * (DashboardView's conditional chrome) stay stable.
 */
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useTimeRange, type TimeRangePreset } from '@/composables/useTimeRange'
import DatePicker from '@/components/common/DatePicker.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const { preset, customFrom, customTo, setPreset, setCustomRange } = useTimeRange()

const PRESETS: { id: TimeRangePreset; key: string }[] = [
  { id: 'today', key: 'dashboard-time-range-today' },
  { id: '7d', key: 'dashboard-time-range-7d' },
  { id: '30d', key: 'dashboard-time-range-30d' },
  { id: '90d', key: 'dashboard-time-range-90d' },
  { id: 'quarter', key: 'dashboard-time-range-quarter' },
]

const open = ref(false)
const customFromInput = ref<string>('')
const customToInput = ref<string>('')
const containerRef = ref<HTMLElement | null>(null)

const triggerLabel = computed(() => {
  if (preset.value === 'custom') {
    if (customFrom.value && customTo.value) {
      return `${customFrom.value} → ${customTo.value}`
    }
    return t('dashboard-time-range-custom')
  }
  const entry = PRESETS.find((p) => p.id === preset.value)
  return entry ? t(entry.key) : t('dashboard-time-range-7d')
})

function togglePopover(): void {
  if (open.value) {
    open.value = false
    return
  }
  // The picker is date-only; take the date part in case an older URL
  // still carries a full datetime value.
  customFromInput.value = customFrom.value?.slice(0, 10) ?? ''
  customToInput.value = customTo.value?.slice(0, 10) ?? ''
  open.value = true
}

function pickPreset(id: TimeRangePreset): void {
  setPreset(id)
  open.value = false
}

function applyCustom(): void {
  if (!customFromInput.value || !customToInput.value) return
  setCustomRange(customFromInput.value, customToInput.value)
  open.value = false
}

function onDocPointerDown(e: PointerEvent): void {
  if (!open.value) return
  const target = e.target as Node | null
  if (target && containerRef.value && containerRef.value.contains(target)) return
  open.value = false
}

function onKeydown(e: KeyboardEvent): void {
  if (e.key === 'Escape' && open.value) {
    open.value = false
  }
}

watch(open, (now) => {
  if (now) {
    document.addEventListener('pointerdown', onDocPointerDown)
    document.addEventListener('keydown', onKeydown)
  } else {
    document.removeEventListener('pointerdown', onDocPointerDown)
    document.removeEventListener('keydown', onKeydown)
  }
})

onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', onDocPointerDown)
  document.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <div ref="containerRef" class="relative inline-flex">
    <button
      type="button"
      :class="[
        'inline-flex items-center gap-1.5 rounded-md border border-default bg-surface px-2 py-1 text-xs text-secondary transition-colors',
        'hover:bg-surface-hover hover:text-primary',
        open ? 'bg-surface-hover text-primary' : '',
      ]"
      :aria-haspopup="true"
      :aria-expanded="open"
      @click="togglePopover"
    >
      <svg viewBox="0 0 24 24" class="w-3.5 h-3.5 text-tertiary" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <circle cx="12" cy="12" r="9" />
        <path d="M12 7v5l3 2" />
      </svg>
      <span class="tabular-nums">{{ triggerLabel }}</span>
      <svg viewBox="0 0 24 24" class="w-3 h-3 text-tertiary" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M6 9l6 6 6-6" />
      </svg>
    </button>

    <div
      v-if="open"
      class="absolute right-0 top-[calc(100%+4px)] z-30 flex flex-col gap-1 rounded-md border border-default bg-surface p-1 shadow-lg min-w-[14rem]"
      role="dialog"
      :aria-label="t('dashboard-time-range-custom')"
    >
      <button
        v-for="p in PRESETS"
        :key="p.id"
        type="button"
        :class="[
          'flex items-center justify-between rounded px-2 py-1.5 text-xs transition-colors',
          preset === p.id ? 'bg-accent-muted text-accent font-medium' : 'text-secondary hover:bg-surface-hover hover:text-primary',
        ]"
        @click="pickPreset(p.id)"
      >
        <span>{{ t(p.key) }}</span>
        <span v-if="preset === p.id" aria-hidden="true">✓</span>
      </button>

      <div class="border-t border-default my-1" />

      <div class="px-2 py-1 flex flex-col gap-2">
        <span class="text-[11px] uppercase tracking-wide text-tertiary font-medium">
          {{ t('dashboard-time-range-custom') }}
        </span>
        <label class="flex flex-col gap-1 text-[11px] text-secondary">
          {{ t('dashboard-time-range-custom-from') }}
          <DatePicker
            v-model="customFromInput"
            :max="customToInput || undefined"
            :aria-label="t('dashboard-time-range-custom-from')"
          />
        </label>
        <label class="flex flex-col gap-1 text-[11px] text-secondary">
          {{ t('dashboard-time-range-custom-to') }}
          <DatePicker
            v-model="customToInput"
            :min="customFromInput || undefined"
            :aria-label="t('dashboard-time-range-custom-to')"
          />
        </label>
        <div class="flex gap-2 justify-end">
          <button
            type="button"
            class="rounded px-2 py-1 text-xs text-secondary hover:bg-surface-hover"
            @click="open = false"
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
  </div>
</template>
