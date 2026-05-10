<script setup lang="ts">
/**
 * PropertyChipRow — header strip + chip-flow shell for the
 * ticket sidebar's property list. Mirrors the visual pattern
 * established by TicketTagsField / TicketWatchersField so the
 * sidebar reads as one coherent property list.
 *
 * The chip-flow is a default slot. Consumers render whatever
 * chip elements they need (static spans, RouterLinks, smart
 * components that fetch their own labels). The shell only owns
 * the section header and the wrapping flex layout for the chips.
 *
 * Empty state collapses to just the header row; when the slot
 * has no rendered output, no chip-row container appears either.
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
  /** Hide the add button — read-only contexts. */
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
    <div class="flex items-center justify-between gap-2">
      <h3 class="text-xs font-medium text-tertiary">{{ label }}</h3>
      <button
        v-if="!readOnly"
        type="button"
        class="p-1 text-tertiary hover:text-accent hover:bg-accent-muted rounded transition-colors print:hidden"
        :title="addLabel"
        :aria-label="addLabel"
        @click="emit('add')"
      >
        <Icon name="add" />
      </button>
    </div>

    <div v-if="hasChipSlot" class="flex flex-wrap items-center gap-1">
      <slot />
    </div>
  </div>
</template>
