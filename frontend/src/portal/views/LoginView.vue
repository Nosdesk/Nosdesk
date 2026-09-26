<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'
import LogoIcon from '@/components/icons/LogoIcon.vue'
import { useBrandingStore } from '@/stores/branding'
import { useThemeStore } from '@/stores/theme'

import { requestMagicLink, signInWithCode } from '../service'

const { $t: t } = useFluent()
const route = useRoute()
const router = useRouter()
const branding = useBrandingStore()
const theme = useThemeStore()

const logoUrl = computed(() => branding.getLogoUrl(theme.isDarkMode))
// The callback redirects here with ?signin_error=1 for a used or expired link.
const linkFailed = computed(() => route.query.signin_error === '1')

const email = ref('')
const sent = ref(false)
const submitting = ref(false)

const code = ref('')
const verifying = ref(false)
const codeFailed = ref(false)

async function submitCode(): Promise<void> {
  if (code.value.replace(/\D/g, '').length !== 6) return
  verifying.value = true
  codeFailed.value = false
  try {
    await signInWithCode(email.value.trim(), code.value)
    void router.replace('/tickets')
  } catch {
    codeFailed.value = true
  } finally {
    verifying.value = false
  }
}

function useAnotherEmail(): void {
  sent.value = false
  code.value = ''
  codeFailed.value = false
}

async function submit(): Promise<void> {
  if (!email.value.trim()) return
  submitting.value = true
  try {
    await requestMagicLink(email.value.trim())
  } finally {
    // Uniform outcome: always "check your email", never revealing whether the
    // address is a known requester.
    sent.value = true
    submitting.value = false
  }
}
</script>

<template>
  <div class="min-h-dvh w-full flex items-center justify-center bg-app p-4">
    <div class="w-full max-w-sm flex flex-col gap-6">
      <div class="flex flex-col items-center gap-4 text-center">
        <img v-if="logoUrl" :src="logoUrl" :alt="branding.appName" class="h-10 max-w-[240px] object-contain" />
        <LogoIcon v-else class="h-10 text-accent" />
        <div class="flex flex-col gap-1">
          <h1 class="text-xl font-semibold text-primary">{{ t('portal-sign-in-title', { app: branding.appName }) }}</h1>
          <p class="text-sm text-secondary">{{ t('portal-sign-in-intro') }}</p>
        </div>
      </div>

      <div class="bg-surface border border-default rounded-xl shadow-sm p-5 flex flex-col gap-4">
        <p v-if="linkFailed && !sent" role="alert" class="text-sm text-status-error">{{ t('portal-sign-in-error') }}</p>
        <template v-if="sent">
          <p class="text-sm text-primary">{{ t('portal-sign-in-sent') }}</p>
          <form class="flex flex-col gap-3" @submit.prevent="submitCode">
            <FormInput
              v-model="code"
              :label="t('portal-code-label')"
              placeholder="123 456"
              inputmode="numeric"
              autocomplete="one-time-code"
              maxlength="7"
              :disabled="verifying"
            />
            <p v-if="codeFailed" role="alert" class="text-sm text-status-error">{{ t('portal-code-invalid') }}</p>
            <Button type="submit" :loading="verifying" :disabled="code.replace(/\D/g, '').length !== 6">
              {{ t('portal-code-submit') }}
            </Button>
          </form>
          <Button variant="ghost" @click="useAnotherEmail">{{ t('portal-sign-in-use-another') }}</Button>
        </template>
        <form v-else class="flex flex-col gap-4" @submit.prevent="submit">
          <FormInput
            v-model="email"
            type="email"
            :label="t('portal-sign-in-email-label')"
            placeholder="you@example.com"
            autocomplete="email"
            required
            :disabled="submitting"
          />
          <Button type="submit" :loading="submitting">{{ t('portal-sign-in-submit') }}</Button>
        </form>
      </div>
    </div>
  </div>
</template>
