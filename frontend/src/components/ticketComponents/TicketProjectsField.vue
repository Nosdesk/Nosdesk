<script setup lang="ts">
/**
 * TicketProjectsField — property-list row for projects this
 * ticket belongs to. Each chip lazily resolves its project name.
 */
import ProjectChip from '@/components/ticketComponents/ProjectChip.vue'
import PropertyChipRow from '@/components/ticketComponents/PropertyChipRow.vue'

defineProps<{
  projectIds: string[]
}>()

const emit = defineEmits<{
  (e: 'add'): void
  (e: 'remove', id: string): void
}>()
</script>

<template>
  <PropertyChipRow
    :label="$t('ticket-field-projects-label')"
    :add-label="$t('ticket-field-projects-add')"
    @add="emit('add')"
  >
    <ProjectChip
      v-for="id in projectIds"
      :key="id"
      :project-id="id"
      @remove="(removedId) => emit('remove', removedId)"
    />
  </PropertyChipRow>
</template>
