<script setup lang="ts">
/**
 * The list's period: which days of work it covers. A quiet trigger while
 * any date goes, and a filter pill (the same chrome as `FilterPill`) once
 * a period is set. It is its own control rather than a facet under "Add
 * filter" because a total only means something beside its period.
 *
 * The panel offers the named periods and one from-to range.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import DatePicker from '@/components/common/DatePicker.vue'
import Icon from '@/components/common/Icon.vue'
import MenuItem from '@/components/common/MenuItem.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import type { PopoverAnchor } from '@/composables/popoverAnchor'
import { formatDay } from '@/components/assets/assetTimeline'
import { PERIOD_PRESETS, type PeriodPreset } from './recordsQuery'

const props = defineProps<{
  period: PeriodPreset | 'custom' | ''
  from: string
  to: string
  /** The newest day a range can end on. */
  max: string
}>()

const emit = defineEmits<{
  (e: 'set-preset', value: PeriodPreset): void
  /** One end of the custom range changed; the other keeps its value. */
  (e: 'set-from', day: string): void
  (e: 'set-to', day: string): void
  (e: 'clear'): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const triggerRef = ref<HTMLElement | null>(null)
const open = ref(false)
const anchor = computed<PopoverAnchor>(() => ({ type: 'element', element: () => triggerRef.value }))

const active = computed(() => props.period !== '')
const summary = computed(() => {
  if (props.period === '') return t('asset-records-period-any')
  if (props.period !== 'custom') return t(`asset-records-period-${props.period}`)
  if (props.from && props.to) {
    return t('asset-records-period-range', { from: formatDay(props.from), to: formatDay(props.to) })
  }
  if (props.from) return t('asset-records-period-from', { from: formatDay(props.from) })
  return t('asset-records-period-until', { to: formatDay(props.to) })
})

function pick(preset: PeriodPreset) {
  emit('set-preset', preset)
  open.value = false
}
</script>

<template>
  <div class="inline-flex min-w-0">
    <div
      ref="triggerRef"
      class="inline-flex min-w-0 items-center h-6 rounded-md border text-2xs overflow-hidden transition-colors"
      :class="active
        ? 'border-accent/40 bg-accent/10 text-accent-on-tint'
        : open
          ? 'border-default bg-surface-hover text-primary'
          : 'border-subtle text-tertiary hover:text-primary hover:border-default hover:bg-surface-hover'"
    >
      <button
        type="button"
        class="inline-flex min-w-0 items-center gap-1 pl-2 h-full"
        :class="active ? 'pr-1.5 hover:bg-accent/15' : 'pr-2'"
        :aria-expanded="open"
        aria-haspopup="dialog"
        @click="open = !open"
      >
        <span v-if="active" class="font-medium">{{ t('asset-records-period-label') }}:</span>
        <span class="truncate">{{ summary }}</span>
        <Icon v-if="!active" name="chevronDown" class="w-3 h-3 shrink-0 opacity-70" />
      </button>
      <button
        v-if="active"
        type="button"
        class="inline-flex items-center justify-center h-full pr-1.5 pl-0.5 hover:bg-accent/15 transition-colors"
        :title="t('asset-records-period-clear')"
        :aria-label="t('asset-records-period-clear')"
        @click="emit('clear')"
      >
        <Icon name="close" class="w-3 h-3" />
      </button>
    </div>

    <ResponsiveMenu
      :open="open"
      :anchor="anchor"
      :title="t('asset-records-period-label')"
      placement="bottom-start"
      react-to-scroll="reposition"
      :offset="4"
      role="dialog"
      :auto-focus="false"
      popover-class="bg-surface border border-default rounded-lg shadow-xl overflow-hidden min-w-[16rem] max-w-[calc(100vw-1rem)]"
      @close="open = false"
    >
      <div role="menu" class="py-1" :aria-label="t('asset-records-period-label')">
        <MenuItem
          v-for="preset in PERIOD_PRESETS"
          :key="preset"
          role="menuitemradio"
          :checked="period === preset"
          @click.stop="pick(preset)"
        >
          <span class="flex-1 text-primary">{{ t(`asset-records-period-${preset}`) }}</span>
          <Icon v-if="period === preset" name="check" class="w-3 h-3 text-accent-ui" />
        </MenuItem>
      </div>
      <div class="flex flex-col gap-1.5 px-3 pt-2 pb-3 border-t border-subtle">
        <span class="text-xs text-secondary">{{ t('asset-records-period-custom') }}</span>
        <DatePicker
          range
          block
          size="sm"
          :aria-label="t('asset-records-period-custom')"
          :start="period === 'custom' ? from : ''"
          :end="period === 'custom' ? to : ''"
          :max="max"
          @update:start="(v: string) => emit('set-from', v)"
          @update:end="(v: string) => emit('set-to', v)"
        />
      </div>
    </ResponsiveMenu>
  </div>
</template>
