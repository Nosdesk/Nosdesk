<script setup lang="ts">
/**
 * Built-in view tab strip for the tickets header.
 *
 * Surfaces the four built-in views (My Open / All Active / Triage
 * / Calendar) as primary navigation rather than burying them in
 * the saved-view dropdown. Calendar in particular needs first-
 * class access — it's a different shape (CalendarBoard vs the
 * list table) and used often enough that one click should land it.
 *
 * Icons are deliberately minimal: a `list` glyph on the list-
 * shape tabs and a `calendar` glyph on the calendar tab. The
 * icon's job is to make the shape change visually obvious, not
 * to disambiguate between the three list-shape slices (the label
 * does that).
 *
 * Saved / project / private views stay behind the smaller
 * `<ViewSwitcher>` dropdown rendered alongside this strip — the
 * tabs are for the well-known set everyone uses; the dropdown is
 * for the tail (user-curated subsets, project-scoped views).
 *
 * Mobile: hides below `sm:` because four 90px tabs don't fit on
 * a phone-width header. Below that breakpoint the parent should
 * render `<ViewSwitcher>` instead, which collapses everything
 * (built-ins + saved) into one popover.
 */
import Icon from '@/components/common/Icon.vue'
import type { IconName } from '@/components/common/icons'

export interface ViewTabItem {
  id: string
  name: string
  /** Icon name from the central registry. Picked for the SHAPE,
   * not the slice — list-shape views all carry the `list` icon,
   * calendar-shape views carry `calendar`. */
  icon: IconName
}

defineProps<{
  items: readonly ViewTabItem[]
  activeId: string
}>()

const emit = defineEmits<{
  (e: 'select', id: string): void
}>()
</script>

<template>
  <!-- Hidden on phones (sm: shows the dropdown fallback in the
       parent). On tablet+ it's a horizontal strip with active-
       state accent painting. Inline-flex so the row doesn't
       force a min-height; the parent header controls vertical
       rhythm. -->
  <div
    class="hidden sm:inline-flex items-center gap-0.5 rounded-md bg-surface-alt p-0.5"
    role="tablist"
    aria-label="View"
  >
    <button
      v-for="item in items"
      :key="item.id"
      type="button"
      role="tab"
      :aria-selected="item.id === activeId"
      class="inline-flex items-center gap-1.5 px-2.5 h-7 rounded text-sm font-medium transition-colors"
      :class="item.id === activeId
        ? 'bg-surface text-primary shadow-sm'
        : 'text-secondary hover:text-primary hover:bg-surface/60'"
      @click="emit('select', item.id)"
    >
      <Icon :name="item.icon" class="w-3.5 h-3.5" aria-hidden="true" />
      <span>{{ item.name }}</span>
    </button>
  </div>
</template>
