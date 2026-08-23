<script setup lang="ts">
/**
 * Gantt view-controls toolbar. Rendered inside the project tab bar's
 * actions slot, but owns its own responsive behaviour so navigation and
 * view-controls stop fighting for one row.
 *
 * It responds to the *container* width (the project panel), not the
 * viewport, so it stays correct regardless of the nav sidebar's state.
 * This matches the `@container` convention already used in
 * TicketDetails / TicketsTable; the `@container` context is the
 * ProjectGanttView root.
 *
 * Wide (>= @2xl container): full controls inline.
 * Cramped (< @2xl): zoom stays inline; Fit / Today fold into a "more
 * controls" overflow menu, and the pan arrows drop out (horizontal
 * scroll covers panning at narrow widths).
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import MenuList, { type MenuItem } from '@/components/common/MenuList.vue'
import {
  GANTT_ZOOMS,
  ganttZoomLabel,
  useGanttViewport,
} from '@/composables/useGanttViewport'
import type { GroupOption } from '@/composables/useListGrouping'
import type { UseGanttSort } from '@/composables/useGanttSort'
import type { Density } from '@/composables/useTicketsDensity'
import ListDensityToggle from '@/components/common/ListDensityToggle.vue'
import type { ComputedRef, Ref } from 'vue'

/** The slice of useListGrouping the picker needs (structural, so
 * the toolbar stays agnostic of the grouped item type). */
interface GroupingControls {
  groupBy: Ref<string>
  setGroupBy: (axisKey: string) => void
  axisOptions: ComputedRef<GroupOption[]>
}

const props = defineProps<{
  viewport: ReturnType<typeof useGanttViewport>
  grouping?: GroupingControls
  sort?: UseGanttSort
  density?: Density
  setDensity?: (value: Density) => void
}>()

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

const isOpen = ref(false)
const triggerRef = ref<HTMLElement | null>(null)
const anchor = computed(() => ({
  type: 'element' as const,
  element: () => triggerRef.value,
}))

const overflowItems = computed<MenuItem[]>(() => [
  { id: 'fit', label: t('gantt-fit') },
  { id: 'today', label: t('gantt-today') },
])

function handleSelect(id: string): void {
  if (id === 'fit') props.viewport.fitToProject()
  else if (id === 'today') props.viewport.centerOnToday()
  isOpen.value = false
}
</script>

<template>
  <div class="flex items-center gap-2">
    <!-- Group-by picker -->
    <BaseDropdown
      v-if="grouping"
      :model-value="grouping.groupBy.value"
      :options="grouping.axisOptions.value.map((o) => ({ value: o.key, label: o.label }))"
      size="xs"
      class="w-32"
      :aria-label="$t('gantt-group-by')"
      @update:model-value="(v) => grouping!.setGroupBy(v as string)"
    />

    <!-- Row-sort picker (applies within the active grouping) -->
    <BaseDropdown
      v-if="sort"
      :model-value="sort.sortBy.value"
      :options="sort.options.value"
      size="xs"
      class="w-32"
      :aria-label="$t('gantt-sort-by')"
      @update:model-value="(v) => sort!.setSortBy(v as string)"
    />

    <!-- Zoom: always inline -->
    <div class="flex items-center rounded-md border border-subtle overflow-hidden">
      <button
        v-for="z in GANTT_ZOOMS"
        :key="z"
        type="button"
        class="text-xs px-2.5 py-1 min-h-[44px] sm:min-h-0 inline-flex items-center justify-center transition-colors"
        :class="viewport.zoom.value === z
          ? 'bg-accent text-on-accent font-medium'
          : 'text-secondary hover:bg-surface-hover'"
        @click="viewport.setZoom(z)"
      >{{ $t(ganttZoomLabel[z]) }}</button>
    </div>

    <!-- Wide: full controls inline. -->
    <div class="hidden @2xl:flex items-center gap-2">
      <ListDensityToggle
        v-if="density && setDensity"
        :density="density"
        @set-density="setDensity"
      />
      <button
        type="button"
        class="text-xs text-secondary hover:bg-surface-hover rounded-md px-2 py-1 border border-subtle"
        :title="`${$t('gantt-fit')} (F)`"
        @click="viewport.fitToProject()"
      >{{ $t('gantt-fit') }}</button>
      <button
        type="button"
        class="text-xs text-secondary hover:bg-surface-hover rounded-md px-2 py-1 border border-subtle"
        :title="`${$t('gantt-today')} (T)`"
        @click="viewport.centerOnToday()"
      >{{ $t('gantt-today') }}</button>
      <div class="flex items-center gap-1">
        <button
          type="button"
          class="text-secondary hover:bg-surface-hover rounded-md px-2 py-1"
          :aria-label="$t('gantt-pan-previous')"
          :title="$t('gantt-pan-previous')"
          @click="viewport.pan(-1)"
        ><Icon name="chevronLeft" size="xs" /></button>
        <button
          type="button"
          class="text-secondary hover:bg-surface-hover rounded-md px-2 py-1"
          :aria-label="$t('gantt-pan-next')"
          :title="$t('gantt-pan-next')"
          @click="viewport.pan(1)"
        ><Icon name="chevronRight" size="xs" /></button>
      </div>
    </div>

    <!-- Cramped: Fit / Today fold into an overflow menu. -->
    <div class="@2xl:hidden relative">
      <button
        ref="triggerRef"
        type="button"
        class="p-1.5 rounded-md hover:bg-surface-hover transition-colors text-secondary hover:text-primary focus:outline-none focus-visible:ring-2 focus-visible:ring-accent inline-flex items-center justify-center min-h-[44px] min-w-[44px] sm:min-h-0 sm:min-w-0"
        :class="{ 'bg-surface-hover text-primary': isOpen }"
        :title="$t('gantt-more-controls')"
        :aria-label="$t('gantt-more-controls')"
        @click="isOpen = !isOpen"
      >
        <Icon name="more" size="md" />
      </button>
      <ResponsiveMenu
        :open="isOpen"
        :anchor="anchor"
        placement="bottom-end"
        react-to-scroll="reposition"
        role="menu"
        :auto-focus="false"
        popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[160px]"
        @close="isOpen = false"
      >
        <MenuList :items="overflowItems" @select="handleSelect" />
      </ResponsiveMenu>
    </div>
  </div>
</template>
