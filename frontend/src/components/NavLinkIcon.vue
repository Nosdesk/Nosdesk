<!--
NavLinkIcon — renders a nav link's icon. Core links carry an SVG path (`icon`);
plugin nav items carry an icon URL (`iconUrl`, rendered as an <img>). Keeps the
img-vs-svg branch in one place across the sidebar's render variants.
-->
<script setup lang="ts">
defineProps<{
  /** SVG path data for a core nav link. */
  icon: string;
  /** Icon URL for a plugin nav item; when set, wins over `icon`. */
  iconUrl?: string;
}>();
</script>

<template>
  <img v-if="iconUrl" :src="iconUrl" class="w-4 h-4 rounded-sm object-cover" alt="" />
  <svg v-else class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor">
    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" :d="icon" />
  </svg>
</template>
