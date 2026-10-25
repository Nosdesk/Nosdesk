<script setup lang="ts">
import { onMounted } from 'vue'
import AuthCallbackCard from '@/components/auth/AuthCallbackCard.vue'

// An install whose redirect URI names this page (as `.env.example` did) sends
// the provider's answer here. The backend finishes the sign-in, and a
// confirmation popup, as a page load at `/api/auth/oauth/callback`, landing in
// the app or on the sign-in page with the reason, so hand the browser on with
// the provider's query unchanged.
onMounted(() => {
  window.location.replace(`/api/auth/oauth/callback${window.location.search}`)
})
</script>

<template>
  <AuthCallbackCard :loading="true" />
</template>
