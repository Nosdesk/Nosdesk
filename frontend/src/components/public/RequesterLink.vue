<template>
  <!-- Requester pages (sign-in, "my requests") may belong to a different app
       than this one: on a hosted tenant origin the server serves the portal
       for them. Always leave with a full page load so the server decides,
       rather than routing inside this app on a guess about the deployment. -->
  <a :href="to"><slot /></a>
</template>

<script setup lang="ts">
defineProps<{ to: string }>()
</script>
