<script setup lang="ts">
/**
 * An authenticator app as the second step of signing in with a password:
 * set it up with a QR code and a first code, turn it off, or get new
 * recovery codes. Turning off and new codes ask for the password.
 */
import { computed, onMounted, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import AssetImg from '@/components/common/AssetImg.vue'
import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'
import OtpInput from '@/components/common/OtpInput.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import { useMfa } from '@/composables/useMfa'
import { useRecoveryCodesFile } from '@/composables/useRecoveryCodesFile'

const emit = defineEmits<{
  (e: 'success', message: string): void
  (e: 'error', message: string): void
}>()

const { $t: t } = useFluent()
const mfa = useMfa()
const { downloadRecoveryCodes } = useRecoveryCodesFile()

onMounted(() => void mfa.checkMFAStatus())

const code = ref('')
const showSecret = ref(false)
const settingUp = computed(() => !mfa.mfaEnabled.value && mfa.mfaStep.value === 'verify')

async function verify(): Promise<void> {
  if (code.value.length !== 6) return
  try {
    if (!(await mfa.verifyMFAToken(code.value))) {
      emit('error', mfa.error.value ?? t('settings-mfa-verify-invalid-code'))
      return
    }
    await mfa.enableMFA(code.value)
    code.value = ''
    emit('success', t('portal-account-authenticator-on'))
  } catch {
    emit('error', mfa.error.value ?? t('settings-mfa-verify-invalid-code'))
  }
}

// Turning it off and new recovery codes both confirm with the password.
const asking = ref<'off' | 'codes' | null>(null)
const password = ref('')
function ask(action: 'off' | 'codes'): void {
  asking.value = action
  password.value = ''
}
async function confirmWithPassword(): Promise<void> {
  if (!password.value) return
  const action = asking.value
  if (action === 'off') {
    if (await mfa.disableMFA(password.value)) {
      emit('success', t('portal-account-authenticator-off'))
      asking.value = null
    } else {
      emit('error', mfa.error.value ?? t('portal-account-authenticator-failed'))
    }
  } else if (action === 'codes') {
    if (await mfa.regenerateBackupCodes(password.value)) {
      asking.value = null
    } else {
      emit('error', mfa.error.value ?? t('portal-account-authenticator-failed'))
    }
  }
  password.value = ''
}

async function copyCodes(): Promise<void> {
  try {
    await navigator.clipboard.writeText(mfa.backupCodes.value.join('\n'))
    emit('success', t('settings-mfa-copied-button'))
  } catch {
    emit('error', t('settings-mfa-copy-error'))
  }
}
</script>

<template>
  <SectionCard content-padding="p-4 sm:p-6">
    <template #title>{{ t('settings-mfa-title') }}</template>
    <div class="flex flex-col gap-5">
      <!-- Off: offer to set it up. -->
      <div v-if="!mfa.mfaEnabled.value && !settingUp" class="flex flex-wrap items-center justify-between gap-3">
        <p class="text-sm text-secondary">{{ t('settings-mfa-toggle-description-disabled') }}</p>
        <Button size="sm" :loading="mfa.loading.value" @click="mfa.startMFASetup()">
          {{ t('portal-account-authenticator-set-up') }}
        </Button>
      </div>

      <!-- Setting up: scan, then type the first code. -->
      <div v-if="settingUp" class="flex flex-col sm:flex-row gap-6 items-center sm:items-start">
        <div class="shrink-0 bg-white p-3 rounded-xl border border-default">
          <AssetImg
            v-if="mfa.qrCodeUrl.value"
            :src="mfa.qrCodeUrl.value"
            :alt="t('settings-mfa-qr-alt')"
            class="w-40 h-40"
          />
        </div>
        <form class="flex flex-col gap-3 min-w-0 flex-1" @submit.prevent="verify">
          <h3 class="text-sm font-medium text-primary">{{ t('settings-mfa-verify-heading') }}</h3>
          <p class="text-sm text-secondary">{{ t('settings-mfa-verify-instructions') }}</p>
          <!-- Left-aligned under its heading; the input centres itself otherwise. -->
          <div class="self-stretch sm:self-start">
            <OtpInput v-model="code" :aria-label="t('settings-mfa-verify-aria-label')" @complete="verify" />
          </div>
          <div class="flex flex-wrap items-center gap-3">
            <Button type="submit" :disabled="code.length !== 6" :loading="mfa.verifying.value || mfa.loading.value">
              {{ t('settings-mfa-verify-button') }}
            </Button>
            <Button variant="secondary" @click="mfa.resetMFASetup()">{{ t('settings-passkey-modal-cancel') }}</Button>
          </div>
          <button type="button" class="self-start text-xs text-accent hover:underline" @click="showSecret = !showSecret">
            {{ t('settings-mfa-manual-toggle') }}
          </button>
          <p v-if="showSecret" class="text-xs text-secondary">
            {{ t('settings-mfa-manual-instructions') }}
            <code class="block mt-1 font-mono text-sm text-primary break-all select-all">{{ mfa.mfaSecret.value }}</code>
          </p>
        </form>
      </div>

      <!-- On. -->
      <div v-if="mfa.mfaEnabled.value" class="flex flex-col gap-4">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <p class="text-sm text-secondary">{{ t('settings-mfa-toggle-description-enabled') }}</p>
          <div v-if="!asking" class="flex flex-wrap gap-2">
            <Button size="sm" variant="secondary" @click="ask('codes')">
              {{ t('portal-account-authenticator-new-codes') }}
            </Button>
            <Button size="sm" variant="ghost-danger" @click="ask('off')">
              {{ t('portal-account-authenticator-turn-off') }}
            </Button>
          </div>
        </div>
        <form v-if="asking" class="flex flex-col sm:flex-row sm:items-end gap-3" @submit.prevent="confirmWithPassword">
          <FormInput
            v-model="password"
            type="password"
            autocomplete="current-password"
            size="sm"
            class="flex-1"
            :label="t('settings-security-label-current')"
            required
          />
          <div class="flex gap-2">
            <Button
              type="submit"
              :variant="asking === 'off' ? 'danger' : 'primary'"
              :disabled="!password"
              :loading="mfa.loading.value"
            >
              {{ asking === 'off' ? t('portal-account-authenticator-turn-off') : t('portal-account-authenticator-new-codes') }}
            </Button>
            <Button variant="secondary" @click="asking = null">{{ t('settings-passkey-modal-cancel') }}</Button>
          </div>
        </form>
      </div>

      <!-- Recovery codes, once, right after they're made. -->
      <div v-if="mfa.mfaEnabled.value && mfa.backupCodes.value.length" class="flex flex-col gap-3 border-t border-default pt-4">
        <h3 class="text-sm font-medium text-primary">{{ t('settings-mfa-backup-codes-heading') }}</h3>
        <p class="text-sm text-secondary">{{ t('settings-mfa-backup-codes-description') }}</p>
        <div class="grid grid-cols-2 sm:grid-cols-4 gap-2">
          <code
            v-for="c in mfa.backupCodes.value"
            :key="c"
            class="rounded-md border border-subtle bg-surface-alt px-2 py-1.5 text-center font-mono text-sm tracking-wider text-primary select-all break-all"
          >{{ c }}</code>
        </div>
        <div class="flex gap-2">
          <Button size="sm" variant="secondary" icon="copy" @click="copyCodes">{{ t('settings-mfa-copy-button') }}</Button>
          <Button size="sm" variant="secondary" icon="download" @click="downloadRecoveryCodes(mfa.backupCodes.value)">
            {{ t('settings-mfa-backup-codes-download') }}
          </Button>
        </div>
      </div>
    </div>
  </SectionCard>
</template>
