<script setup lang="ts">
/**
 * Built-in view tab strip for the tickets header.
 *
 * Surfaces only the PRIMARY built-in views (My Open / My Active /
 * All Active / Triage — the daily drivers) as one-click tabs. The
 * set is capped at four on purpose: earlier every built-in was a
 * tab, and as the built-in count grew to eight the strip sprawled
 * ~900px wide and crowded the filter / display chrome sharing the
 * row. The remaining built-ins (All Tickets / Unassigned / Overdue
 * / Calendar) now live in the sibling `<ViewSwitcher>` "Views ▾"
 * overflow dropdown.
 *
 * Each tab carries a slice-specific icon (see `TAB_ICON` in
 * `useTicketsViewResolution`) so the active slice is recognisable
 * at a glance, not just by label.
 *
 * Saved / project / private views also live behind the
 * `<ViewSwitcher>` dropdown alongside this strip — the tabs are
 * for the well-known set everyone uses; the dropdown is for the
 * overflow built-ins plus the tail of user-curated subsets.
 *
 * Visibility: only on `lg:+` (1024px). Tabs are a desktop-class
 * affordance; tablet and narrow-laptop widths (sm-md, 640-1024)
 * have enough chrome competing in the header that four labelled
 * tabs end up wrapping their labels or pushing other controls
 * onto a second row. Below lg the parent renders a single
 * `<ViewSwitcher>` dropdown that combines built-ins + saved
 * views into one popover — much cleaner at narrow widths.
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
  <!-- Hidden below lg: (1024px) — narrower viewports use the
       parent's consolidated dropdown instead. `whitespace-nowrap`
       on each label is belt-and-braces against the strip getting
       compressed by sibling header content; without it, "My Open"
       wraps to two lines the moment the row gets tight. -->
  <div
    class="hidden lg:inline-flex items-center gap-0.5 rounded-md bg-surface-alt p-0.5"
    role="tablist"
    :aria-label="$t('views-tab-bar-aria')"
  >
    <button
      v-for="item in items"
      :key="item.id"
      type="button"
      role="tab"
      :aria-selected="item.id === activeId"
      class="inline-flex items-center gap-1.5 px-2.5 h-7 rounded text-sm font-medium transition-colors whitespace-nowrap shrink-0"
      :class="item.id === activeId
        ? 'bg-surface text-primary shadow-sm'
        : 'text-secondary hover:text-primary hover:bg-surface/60'"
      @click="emit('select', item.id)"
    >
      <Icon :name="item.icon" class="w-3.5 h-3.5 shrink-0" aria-hidden="true" />
      <span>{{ item.name }}</span>
    </button>
  </div>
</template>
