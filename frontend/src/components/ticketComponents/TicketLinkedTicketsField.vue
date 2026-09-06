<script setup lang="ts">
/**
 * TicketLinkedTicketsField — property-list row for tickets
 * linked to this one. Each chip lazily resolves its title
 * (LinkedTicketChip handles the per-id fetch). The wrapper
 * also acts as a drop target so dragging another ticket here
 * still links it, preserving the existing power-user flow.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import PropertyChipRow from '@/components/ticketComponents/PropertyChipRow.vue'
import LinkedTicketChip from '@/components/ticketComponents/LinkedTicketChip.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const props = defineProps<{
  linkedTicketIds: number[]
  /** True while the user is dragging a ticket from elsewhere
   *  in the app. Surfaces a drop-affordance pill in the chip
   *  flow so the linkable target is obvious. */
  showDropAffordance?: boolean
  /** True when the drag is currently hovering this drop zone. */
  isDropTarget?: boolean
  /** The ticket being dragged, for the affordance label. */
  dragLabel?: string | null
}>()

const emit = defineEmits<{
  (e: 'add'): void
  (e: 'remove', id: number): void
}>()

const hasContent = computed(
  () => props.linkedTicketIds.length > 0 || props.showDropAffordance || props.isDropTarget,
)
</script>

<template>
  <PropertyChipRow
    :label="t('ticket-field-linked-tickets-label')"
    :add-label="t('ticket-field-linked-tickets-add')"
    :hide-chips="!hasContent"
    @add="emit('add')"
  >
    <span
      v-if="showDropAffordance || isDropTarget"
      class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-2xs font-medium border border-dashed transition-colors"
      :class="isDropTarget
        ? 'border-accent bg-accent/10 text-accent'
        : 'border-accent/40 text-accent/70'"
    >
      {{ isDropTarget && dragLabel ? dragLabel : t('ticket-field-linked-tickets-drop') }}
    </span>

    <LinkedTicketChip
      v-for="id in linkedTicketIds"
      :key="id"
      :ticket-id="id"
      @remove="(removedId) => emit('remove', removedId)"
    />
  </PropertyChipRow>
</template>
