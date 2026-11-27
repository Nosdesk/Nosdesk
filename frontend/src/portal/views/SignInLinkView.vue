<script setup lang="ts">
// Where a sign-in email's link lands. Opening the link changes nothing: mail
// scanners open every link on delivery, and one that signed in would spend the
// link and confirm the address for whoever asked for it. The button does both.
import AssetImg from '@/components/common/AssetImg.vue'
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import LogoIcon from '@/components/icons/LogoIcon.vue'
import { useBrandingStore } from '@/stores/branding'
import { useThemeStore } from '@/stores/theme'

import { signInWithLink } from '../service'
import { SIGNED_IN_HOME } from '../signInRedirect'

const { $t: t } = useFluent()
const route = useRoute()
const router = useRouter()
const branding = useBrandingStore()
const theme = useThemeStore()

const logoUrl = computed(() => branding.getLogoUrl(theme.isDarkMode))
const token = computed(() => (typeof route.query.token === 'string' ? route.query.token : ''))
const signingIn = ref(false)

async function continueSignIn(): Promise<void> {
  if (!token.value) {
    void router.replace({ name: 'login', query: { signin_error: '1' } })
    return
  }
  signingIn.value = true
  try {
    await signInWithLink(token.value)
    void router.replace(SIGNED_IN_HOME)
  } catch {
    void router.replace({ name: 'login', query: { signin_error: '1' } })
  } finally {
    signingIn.value = false
  }
}
</script>

<template>
  <div class="min-h-dvh w-full flex items-center justify-center bg-app p-4">
    <div class="w-full max-w-sm flex flex-col gap-6">
      <div class="flex flex-col items-center gap-4 text-center">
        <AssetImg v-if="logoUrl" :src="logoUrl" :alt="branding.appName" class="h-10 max-w-[240px] object-contain" />
        <LogoIcon v-else class="h-10 text-accent-fill" />
        <div class="flex flex-col gap-1">
          <h1 class="text-xl font-semibold text-primary">{{ t('portal-sign-in-link-title', { app: branding.appName }) }}</h1>
          <p class="text-sm text-secondary">{{ t('portal-sign-in-link-intro') }}</p>
        </div>
      </div>
      <div class="bg-surface border border-default rounded-xl shadow-sm p-5 flex flex-col gap-4">
        <Button variant="primary" block :loading="signingIn" @click="continueSignIn">
          {{ t('portal-sign-in-link-continue') }}
        </Button>
      </div>
    </div>
  </div>
</template>
