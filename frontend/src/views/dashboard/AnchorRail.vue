<script setup lang="ts">
/**
 * Left-sticky section anchor rail.
 *
 * Renders the seven canonical section anchors as a vertical list;
 * the currently-active section (the one whose marker is closest to
 * the viewport top) highlights. Clicking an anchor smooth-scrolls
 * the marker into view via useAnchorScroll.
 *
 * The rail is decoupled from canvas content: it lists every
 * planned section regardless of whether that section has any
 * widgets yet. Phase 12 (Wave 8) seeds default widgets per section;
 * until then, empty sections still register their markers so the
 * rail's click-to-scroll lands somewhere reasonable.
 *
 * The keyboard shortcut "1"-"7" (registered by
 * useDashboardKeybindings in a later wave) maps directly to the
 * SECTIONS array index, so reordering this list reorders the
 * shortcuts too. Be deliberate about changes here.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import type { AnchorScroll } from '@/composables/useAnchorScroll'

const props = defineProps<{
  anchorScroll: AnchorScroll
}>()

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

import { SECTIONS } from './sections'

const activeId = computed(() => props.anchorScroll.activeId.value)

function jump(id: string): void {
  props.anchorScroll.scrollTo(id)
}
</script>

<template>
  <nav
    class="sticky top-4 hidden flex-col gap-0.5 self-start text-xs xl:flex"
    :aria-label="t('dashboard-anchor-rail-aria-label')"
  >
    <button
      v-for="section in SECTIONS"
      :key="section.id"
      type="button"
      :class="[
        'rounded-md px-2 py-1.5 text-left transition-colors',
        activeId === section.id
          ? 'bg-accent/10 text-accent font-medium'
          : 'text-secondary hover:bg-surface-hover hover:text-primary',
      ]"
      :aria-current="activeId === section.id ? 'true' : undefined"
      @click="jump(section.id)"
    >
      {{ t(section.labelKey) }}
    </button>
  </nav>
</template>
