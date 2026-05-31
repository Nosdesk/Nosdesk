<script setup lang="ts">
/**
 * PropertyChipRow — header strip + chip-flow shell for the
 * ticket sidebar's property list. The header is the click
 * target for the section's add affordance: the entire label
 * row is a button with a subtle hover-bg lift and a `+` glyph
 * that reveals on hover. No discrete + button in the default
 * state, so an empty sidebar reads as a clean column of
 * labels rather than a "clump of plus buttons" on the right.
 *
 * Mirrors the visual pattern in TicketTagsField /
 * TicketWatchersField so the entire sidebar reads as one
 * coherent property list.
 *
 * Empty state collapses to just the header row (no chip-row
 * container appears either).
 */
import { Comment, Fragment, computed, useSlots, type VNode } from 'vue'
import Icon from '@/components/common/Icon.vue'

const props = defineProps<{
  label: string
  addLabel: string
  /** When true, set by callers that have their own way of
   *  reflecting non-empty state (e.g. counts in the label).
   *  Suppresses the chip-row container even if the slot is
   *  rendered, useful when the caller only wants the header. */
  hideChips?: boolean
  /** Hide the add affordance — read-only contexts. The header
   *  becomes a plain label, not a button. */
  readOnly?: boolean
}>()

const emit = defineEmits<{
  (e: 'add'): void
}>()

const slots = useSlots()

// Auto-detect whether the slot renders anything visible. Without
// this, a row whose only slot content is a `v-for` over an empty
// list (Assets / Projects / Documentation when empty) still mounts
// the chip-row container `<div>`, and the parent's `gap-1` between
// the heading button and that empty div adds a phantom 4px that
// the same row WITH `:hide-chips` (Linked Tickets / Tags) doesn't
// have. The result was a Cluster D rhythm where empty rows were
// 28px or 24px depending on whether the consumer remembered to
// pass `:hide-chips`. Auto-detection makes the row a uniform 24px
// regardless of whether the consumer passes the prop.
//
// The check recursively unwraps Fragment vnodes (v-for / template
// wrappers) and treats Comment vnodes (v-if=false) and whitespace-
// only text as empty. Anything else counts as a real render.
function hasRenderedContent(vnode: VNode): boolean {
  if (vnode.type === Comment) return false
  if (vnode.type === Fragment) {
    return Array.isArray(vnode.children)
      ? vnode.children.some((c) => hasRenderedContent(c as VNode))
      : false
  }
  if (typeof vnode.children === 'string') {
    return vnode.children.trim().length > 0
  }
  return true
}

const hasChipSlot = computed(() => {
  if (props.hideChips) return false
  if (!slots.default) return false
  return slots.default().some(hasRenderedContent)
})
</script>

<template>
  <!-- gap-1 (4px) hugs the chip row to its heading. Sits inside a
       cluster that uses gap-2 (8px) between sibling rows, so
       label+chips reads as one unit. See TicketDetails for the
       1:2:3 (4/8/12px) spacing scale. -->
  <div class="flex flex-col gap-1">
    <!-- Whole label row is the click target for the picker.
         Negative-margin idiom: `-mx-2` extends the button 8px
         past the property-list container's content edge on
         each side (the container reserves that 8px via its
         own `px-2`, balanced by SectionCard's reduced
         horizontal padding — see TicketDetails for the
         layered explanation). `px-2` adds the same 8px back as
         internal padding so the label TEXT sits at the
         identical x as plain <h3> siblings (Watchers /
         Resolution / Status). The negative margin is fully
         honored because the parent reserves the room for it.
         `py-1` adds vertical hit area + hover-bg breathing
         room without changing the horizontal alignment. -->
    <button
      v-if="!readOnly"
      type="button"
      class="group flex items-center justify-between gap-2 -mx-2 px-2 py-1 rounded text-left hover:bg-surface-hover transition-colors print:hidden"
      :title="addLabel"
      :aria-label="addLabel"
      @click="emit('add')"
    >
      <h3 class="text-xs font-medium text-tertiary group-hover:text-secondary transition-colors">{{ label }}</h3>
      <!-- `+` at low opacity always so empty Relations rows read as
           "click to add" rather than as decorative labels. Bumps to
           full opacity on hover for affordance reinforcement. -->
      <Icon
        name="add"
        class="w-3.5 h-3.5 text-tertiary opacity-40 group-hover:opacity-100 transition-opacity"
        aria-hidden="true"
      />
    </button>
    <div v-else class="flex items-center justify-between gap-2">
      <h3 class="text-xs font-medium text-tertiary">{{ label }}</h3>
    </div>

    <!-- Chip flow: also negative-margined so the row's structural
         footprint matches the heading button's. The chips
         themselves still sit at the same x as plain content
         (the inner `px-2` cancels the outer `-mx-2`); only the
         container extends into the breathing area, matching the
         button above. Without this the button row is visually
         8px wider than the chip row below it. -->
    <div v-if="hasChipSlot" class="flex flex-wrap items-center gap-1 -mx-2 px-2">
      <slot />
    </div>
  </div>
</template>
