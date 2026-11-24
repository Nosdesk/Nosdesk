<script setup lang="ts">
/**
 * The size choices: three presets and none. One menu for every place a
 * size is set (the chip on a row, the Size field), so the wording and the
 * lengths never differ. A popover from `md`, a bottom sheet below it.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import MenuList, { type MenuItem } from '@/components/common/MenuList.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import type { PopoverAnchor, PopoverPlacement } from '@/composables/popoverAnchor'
import { SIZE_MINUTES, SIZES, sizeOf, type Size } from '@nosdesk/core/utils/ticketPlan'

const props = withDefaults(
  defineProps<{
    open: boolean
    anchor: PopoverAnchor
    minutes: number | null | undefined
    placement?: PopoverPlacement
    matchAnchorWidth?: boolean
  }>(),
  { placement: 'bottom-end', matchAnchorWidth: false },
)

const emit = defineEmits<{
  (e: 'select', minutes: number | null): void
  (e: 'close'): void
}>()

const fluent = useFluent()

const items = computed<MenuItem[]>(() => {
  const current = sizeOf(props.minutes)
  return [
    ...SIZES.map((size) => ({
      id: size,
      label: fluent.$t(`plan-size-${size}`),
      trailing: fluent.$t(`plan-size-length-${size}`),
      checked: current === size,
    })),
    { id: 'none', label: fluent.$t('plan-size-none'), checked: current === null, divider: true },
  ]
})

function onSelect(id: string): void {
  emit('select', id === 'none' ? null : SIZE_MINUTES[id as Size])
  emit('close')
}
</script>

<template>
  <ResponsiveMenu
    :open="open"
    :anchor="anchor"
    :title="$t('plan-size-label')"
    :placement="placement"
    :match-anchor-width="matchAnchorWidth"
    :min-width="176"
    popover-class="bg-surface border border-default rounded-lg shadow-lg py-1"
    role="menu"
    @close="emit('close')"
  >
    <MenuList :items="items" @select="onSelect" />
  </ResponsiveMenu>
</template>
