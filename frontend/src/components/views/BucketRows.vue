<script setup lang="ts">
/**
 * Inner row loop for the grouped TicketsTable.
 *
 * Extracted from TicketsTable so v-memo's per-instance cache slots
 * are scoped to one bucket at a time. Vue's v-memo cache is keyed
 * by compile-time slot index, not by outer-v-for iteration, so a
 * v-memo placed directly inside an outer v-for would have all
 * outer iterations sharing one cache and silently corrupt each
 * other's memoization. Moving the inner loop into its own
 * component gives every bucket its own instance + cache.
 */
import TicketRow from '@/components/views/TicketRow.vue'
import { rowMemoKey, type ListColumn } from '@/sync/views/ticketColumns'
import type { CardData } from '@/sync/views/types'
import type { BulkSelection } from '@/composables/useBulkSelection'

defineProps<{
  cards: CardData[]
  visibleColumns: ListColumn[]
  rowClass: string
  cellPadding: string
  colStyle: (col: ListColumn) => Record<string, string>
  selectedId?: number | null
  bulkActive: boolean
  bulkSelection?: BulkSelection<CardData>
}>()

defineEmits<{
  (e: 'click', id: number): void
  (e: 'toggle-bulk', id: number, shiftKey: boolean): void
}>()
</script>

<template>
  <TicketRow
    v-for="card in cards"
    :key="card.id"
    v-memo="[
      ...rowMemoKey(card),
      visibleColumns,
      cellPadding,
      rowClass,
      selectedId === card.id,
      bulkActive,
      bulkSelection?.isSelected(String(card.id)) ?? false,
    ]"
    :card="card"
    :visible-columns="visibleColumns"
    :row-class="rowClass"
    :cell-padding="cellPadding"
    :col-style="colStyle"
    :selected="selectedId === card.id"
    :bulk-active="bulkActive"
    :bulk-selected="bulkSelection?.isSelected(String(card.id)) ?? false"
    @click="(id: number) => $emit('click', id)"
    @toggle-bulk="(id: number, shiftKey: boolean) => $emit('toggle-bulk', id, shiftKey)"
  />
</template>
