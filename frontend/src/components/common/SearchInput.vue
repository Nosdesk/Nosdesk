<script setup lang="ts">
/**
 * Non-debounced search input. Use this when the list being filtered
 * is small enough that every keystroke can drive a synchronous,
 * client-side filter (canned-response admin list, etc.). For larger
 * lists or remote queries, use `DebouncedSearchInput` instead.
 *
 * Visual treatment mirrors `DebouncedSearchInput`: leading icon,
 * bordered shell, accent focus ring, hover-darkens-border. Keeps
 * search inputs across the app visually consistent regardless of
 * whether the consumer needs debouncing.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'

const props = defineProps<{
  modelValue: string
  placeholder?: string
}>()

const fluent = useFluent()
const resolvedPlaceholder = computed(() => props.placeholder ?? fluent.$t('common-search-placeholder'))

const emit = defineEmits<{
  'update:modelValue': [value: string]
}>()

const localSearchQuery = ref(props.modelValue)

watch(() => props.modelValue, (newValue) => {
  localSearchQuery.value = newValue
})

watch(localSearchQuery, (newValue) => {
  emit('update:modelValue', newValue)
})
</script>

<template>
  <div class="relative flex-grow min-w-[150px]">
    <div class="absolute inset-y-0 left-0 flex items-center pl-2 pointer-events-none text-tertiary">
      <Icon name="search" />
    </div>
    <input
      v-model="localSearchQuery"
      type="search"
      :placeholder="resolvedPlaceholder"
      class="block w-full py-1.5 pl-8 pr-2 text-sm rounded-lg bg-surface-alt border border-default text-primary placeholder-tertiary transition-colors duration-200 hover:border-strong focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
    />
  </div>
</template>
