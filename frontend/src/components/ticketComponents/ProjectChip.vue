<script setup lang="ts">
/**
 * ProjectChip — chip wrapper that resolves a project id to its
 * name for the property-list surface. Falls back to `Project
 * #{id}` while loading.
 */
import { ref, watch } from 'vue'
import projectService from '@/services/projectService'
import PropertyChip from '@/components/ticketComponents/PropertyChip.vue'

const props = defineProps<{
  projectId: string
}>()

const emit = defineEmits<{
  (e: 'remove', id: string): void
}>()

const name = ref<string | null>(null)
const loading = ref(true)

watch(
  () => props.projectId,
  async (id) => {
    loading.value = true
    name.value = null
    try {
      const fetched = await projectService.getProject(Number(id))
      name.value = fetched?.name ?? null
    } catch {
      name.value = null
    } finally {
      loading.value = false
    }
  },
  { immediate: true },
)
</script>

<template>
  <PropertyChip
    :label="name || `Project #${projectId}`"
    :title="name || `Project #${projectId}`"
    :to="`/projects/${projectId}`"
    :loading="loading"
    removable
    remove-title="Remove from project"
    @remove="emit('remove', projectId)"
  />
</template>
