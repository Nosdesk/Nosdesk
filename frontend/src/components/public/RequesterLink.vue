<template>
  <!-- Inside the portal, requester pages are this app's own routes. Elsewhere
       they may belong to a different app (on a hosted tenant origin the server
       serves the portal for them), so leave with a full page load and let the
       server decide rather than routing here on a guess. -->
  <RouterLink v-if="inPortal" :to="to"><slot /></RouterLink>
  <a v-else :href="to"><slot /></a>
</template>

<script setup lang="ts">
import { RouterLink } from 'vue-router'

import { useInPortal } from './inPortal'

defineProps<{ to: string }>()
const inPortal = useInPortal()
</script>
