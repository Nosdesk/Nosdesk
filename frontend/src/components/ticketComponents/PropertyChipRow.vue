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
import { computed, useSlots } from 'vue'
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
const hasChipSlot = computed(() => !!slots.default && !props.hideChips)
</script>

<template>
  <div class="flex flex-col gap-1.5">
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
      <Icon
        name="add"
        class="w-3.5 h-3.5 text-tertiary opacity-0 group-hover:opacity-100 transition-opacity"
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
