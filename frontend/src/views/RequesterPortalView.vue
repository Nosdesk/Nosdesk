<!-- RequesterPortalView.vue
     Where a requester signed in to the native app lands on a self-hosted
     server: their requests live in the portal, which opens in the browser
     already signed in (a one-time hand-off, asked for on each press). -->
<script setup lang="ts">
import { ref } from 'vue'
import { useFluent } from 'fluent-vue'

import AuthLayout from '@/components/auth/AuthLayout.vue'
import Button from '@/components/common/Button.vue'
import { openExternalUrl } from '@/platform'
import { useAuthStore } from '@/stores/auth'
import { extractErrorMessage } from '@/utils/errors'
import { requestPortalHandoff } from '@nosdesk/core/services/workspacePortalService'

const { $t: t } = useFluent()
const auth = useAuthStore()
const opening = ref(false)
const failed = ref<string | null>(null)

async function openPortal(): Promise<void> {
  opening.value = true
  failed.value = null
  try {
    await openExternalUrl(await requestPortalHandoff())
  } catch (e) {
    failed.value = extractErrorMessage(e, t('requester-portal-open-failed'))
  } finally {
    opening.value = false
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
      <p v-if="failed" role="alert" class="text-sm text-status-error">{{ failed }}</p>
      <div class="flex flex-col gap-3">
        <Button size="lg" block :loading="opening" @click="openPortal">{{ t('requester-portal-open') }}</Button>
        <Button size="lg" block variant="secondary" @click="auth.logout()">{{ t('requester-portal-sign-out') }}</Button>
      </div>
    </div>
  </AuthLayout>
</template>
