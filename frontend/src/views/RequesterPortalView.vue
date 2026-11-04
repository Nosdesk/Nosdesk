<!-- RequesterPortalView.vue
     Where a requester who signed in at the native app lands on a self-hosted
     server. The app is for staff, so they hold no session in it: their sign-in
     opened the portal in the browser, already signed in, and this screen opens
     it there again. -->
<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'

import AuthLayout from '@/components/auth/AuthLayout.vue'
import Button from '@/components/common/Button.vue'
import { openExternalUrl } from '@/platform'
import { isWebAddress } from '@/utils/portalLanding'

const { $t: t } = useFluent()
const route = useRoute()
const router = useRouter()
const failed = ref(false)

/** The portal's address, from the sign-in that landed here. */
const portalUrl = computed(() => {
  const portal = route.query.portal
  return isWebAddress(portal) ? portal : null
})

async function openPortal(): Promise<void> {
  if (!portalUrl.value) return
  failed.value = false
  try {
    await openExternalUrl(portalUrl.value)
  } catch {
    failed.value = true
  }
}
</script>

<template>
  <AuthLayout>
    <div class="flex flex-col gap-6">
      <div class="flex flex-col gap-2 text-center lg:text-left">
        <h1 class="text-2xl sm:text-3xl font-semibold tracking-tight text-primary">
          {{ t('requester-portal-title') }}
        </h1>
        <p class="text-base text-secondary text-pretty">{{ t('requester-portal-message') }}</p>
      </div>
      <p v-if="failed" role="alert" class="text-sm text-status-error">{{ t('requester-portal-open-failed') }}</p>
      <div class="flex flex-col gap-3">
        <Button v-if="portalUrl" size="lg" block @click="openPortal">{{ t('requester-portal-open') }}</Button>
        <Button size="lg" block variant="secondary" @click="router.replace({ name: 'login' })">
          {{ t('requester-portal-another-account') }}
        </Button>
      </div>
    </div>
  </AuthLayout>
</template>
