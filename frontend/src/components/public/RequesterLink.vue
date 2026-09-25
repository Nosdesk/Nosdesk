<template>
  <!-- On a hosted tenant origin the requester pages (sign-in, "my requests")
       belong to the portal, a separate app the server picks per path, so leave
       the agent app with a full page load. Self-hosted stays in-app. -->
  <a v-if="isHosted" :href="to"><slot /></a>
  <RouterLink v-else :to="to"><slot /></RouterLink>
</template>

<script setup lang="ts">
import { RouterLink } from 'vue-router'
import { isHostedDeployment } from '@nosdesk/core/services/instanceConfig'

defineProps<{ to: string }>()

const isHosted = isHostedDeployment()
</script>
