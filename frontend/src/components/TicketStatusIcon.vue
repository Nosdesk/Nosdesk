<!--
Tiny progress-state icon for a ticket, in the same vocabulary as
GitHub's PR status glyphs: empty circle for open, half-filled for in
progress, filled-with-check for closed. Colors come from the
`text-status-*` tokens so the icon inherits theme colors automatically
and stays coherent with the tone dots used in the status dropdown.
-->
<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  status: 'open' | 'in-progress' | 'closed' | string
  /** Optional title override for native tooltip. Defaults to the
   *  status label (capitalised, hyphen replaced with space). */
  title?: string
}>()

const label = computed(() => {
  if (props.title) return props.title
  if (props.status === 'in-progress') return 'In progress'
  return props.status.charAt(0).toUpperCase() + props.status.slice(1)
})

const toneClass = computed(() => {
  switch (props.status) {
    case 'open':
      return 'text-status-open'
    case 'in-progress':
      return 'text-status-in-progress'
    case 'closed':
      return 'text-status-closed'
    default:
      return 'text-tertiary'
  }
})
</script>

<template>
  <svg
    :class="['flex-shrink-0', toneClass]"
    viewBox="0 0 24 24"
    :aria-label="label"
    role="img"
  >
    <title>{{ label }}</title>

    <!-- Open: empty outlined circle — "not yet started" -->
    <template v-if="status === 'open'">
      <circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" stroke-width="2.5" />
    </template>

    <!-- In progress: circle outline + half-fill arc, reads as a
         progress wheel mid-rotation -->
    <template v-else-if="status === 'in-progress'">
      <circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" stroke-width="2.5" />
      <path d="M12 4 A 8 8 0 0 1 12 20 Z" fill="currentColor" />
    </template>

    <!-- Closed: filled circle with a white check — "done" -->
    <template v-else-if="status === 'closed'">
      <circle cx="12" cy="12" r="9" fill="currentColor" />
      <path
        d="M8 12 L11 15 L16 9"
        fill="none"
        stroke="white"
        stroke-width="2.5"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </template>
  </svg>
</template>
