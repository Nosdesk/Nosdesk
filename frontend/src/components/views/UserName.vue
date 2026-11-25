<script setup lang="ts">
/**
 * A person's name, resolved through the users directory, as plain text
 * that wraps. Its own component for the same reason as `UserCell`: it
 * keeps its own reactive scope, so the name arrives even inside a row
 * under `v-memo`.
 */
import { computed } from 'vue'
import { useUsersDirectory } from '@/composables/useUsersDirectory'

const props = defineProps<{ uuid: string }>()

const { getUserHandle } = useUsersDirectory()
const name = computed(() => getUserHandle(props.uuid).user.value?.name ?? '')
</script>

<template>
  <span v-if="name" class="[overflow-wrap:anywhere]">{{ name }}</span>
</template>
