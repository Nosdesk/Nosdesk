<script setup lang="ts">
import SearchInput from './SearchInput.vue'

defineProps<{
  title: string
  searchQuery: string
  addButtonText?: string
  showAddButton?: boolean
}>()

const emit = defineEmits<{
  'update:searchQuery': [value: string]
  'add': []
}>()
</script>

<template>
  <div class="flex flex-col md:flex-row md:items-center justify-between mb-6 gap-4">
    <h1 class="text-2xl font-medium text-primary">{{ title }}</h1>
    <div class="flex flex-col md:flex-row items-stretch md:items-center gap-4">
      <SearchInput
        :model-value="searchQuery"
        @update:model-value="(value) => emit('update:searchQuery', value)"
        :placeholder="`Search ${title.toLowerCase()}...`"
      />
      <button
        v-if="showAddButton !== false"
        @click="emit('add')"
        class="bg-blue-500 hover:bg-blue-600 text-primary px-4 py-2 rounded-lg transition-colors"
      >
        {{ addButtonText || `Add ${title.slice(0, -1)}` }}
      </button>
    </div>
  </div>
</template> 