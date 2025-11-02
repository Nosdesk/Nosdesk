<script setup lang="ts">
interface Props {
  id: string | number
  prefix?: string
}

const props = withDefaults(defineProps<Props>(), {
  prefix: '#'
})
</script>

<template>
  <span class="text-secondary text-sm">
    {{ prefix }}{{ id }}
  </span>
</template> 