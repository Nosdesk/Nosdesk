<script setup lang="ts">
interface Props {
  value: string | number
  fontWeight?: 'normal' | 'medium' | 'bold'
  color?: 'primary' | 'secondary' | 'muted'
}

const props = withDefaults(defineProps<Props>(), {
  fontWeight: 'normal',
  color: 'primary'
})

const colorClasses = {
  primary: 'text-primary',
  secondary: 'text-secondary',
  muted: 'text-tertiary'
}

const fontWeightClasses = {
  normal: 'font-normal',
  medium: 'font-medium',
  bold: 'font-bold'
}
</script>

<template>
  <span :class="[
    'truncate', 
    colorClasses[color],
    fontWeightClasses[fontWeight]
  ]">
    {{ value }}
  </span>
</template> 