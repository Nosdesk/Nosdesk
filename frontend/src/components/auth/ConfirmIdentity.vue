<!--
"Confirm it's you" inside a high-risk action's dialog. The caller keeps its
own title and body copy and its confirm button; this asks for whatever proof
the server will take (see useStepUp) and tells the caller when the button may
go live through `v-model:ready`.

Caller contract:
  <ConfirmIdentity ref="identity" v-model:ready="identityReady" @submit="run" />
  confirm button disabled while `!identityReady`; on click:
    if (!(await identity.value?.confirm())) return   // error shows inline
    ...run the action with an empty credential...

`ready` is true once the session is confirmed, or once a password or code is
typed (`confirm()` then sends it first). A passkey or a provider sign-in
confirms on its own button, after which `ready` follows.
-->
<script setup lang="ts">
import { computed, ref, useId, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import { useStepUp } from '@/composables/useStepUp';
import type { StepUpMethod } from '@/services/stepUpService';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';
import PasswordInput from '@/components/common/PasswordInput.vue';
import Icon from '@/components/common/Icon.vue';

const emit = defineEmits<{
  (e: 'update:ready', ready: boolean): void;
  /** Enter in the password or code field. */
  (e: 'submit'): void;
}>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const stepUp = useStepUp();
const {
  loading,
  loadFailed,
  confirmed,
  methods,
  provider,
  email,
  windowMinutes,
  busy,
  error,
  waiting,
  popupResult,
} = stepUp;

const fieldId = useId();

// The method on screen: the server's first choice until the user picks
// another, and back to it if the picked one disappears.
const picked = ref<StepUpMethod | null>(null);
const method = computed<StepUpMethod | null>(() =>
  picked.value && methods.value.includes(picked.value) ? picked.value : (methods.value[0] ?? null),
);
const otherMethods = computed(() => methods.value.filter((m) => m !== method.value));
const showOtherMethods = ref(false);

const credential = ref('');
const typesCredential = computed(() => method.value === 'password' || method.value === 'code');

const ready = computed(
  () =>
    !loading.value &&
    (confirmed.value || (typesCredential.value && credential.value.trim().length > 0)),
);
watch(ready, (value) => emit('update:ready', value), { immediate: true });

const providerName = computed(() => provider.value?.name ?? '');

function methodChoiceLabel(m: StepUpMethod): string {
  switch (m) {
    case 'password':
      return t('step-up-choice-password');
    case 'code':
      return t('step-up-choice-code');
    case 'passkey':
      return t('step-up-passkey-button');
    case 'provider':
      return t('step-up-provider-button', { name: providerName.value });
  }
}

function pickMethod(m: StepUpMethod) {
  picked.value = m;
  showOtherMethods.value = false;
  credential.value = '';
  stepUp.clearMessages();
  if (m !== 'provider') stepUp.cancelWaiting();
  // Choosing a passkey or the provider is choosing to use it now. The popup
  // must open inside this click, before anything is awaited.
  if (m === 'provider') stepUp.openProviderPopup();
  else if (m === 'passkey') void stepUp.confirmWithPasskey();
}

/** Opens the provider sign-in. Synchronous on purpose: see openProviderPopup. */
function continueWithProvider() {
  stepUp.openProviderPopup();
}

const message = computed<{ text: string; tone: 'error' | 'quiet' } | null>(() => {
  if (error.value) return { text: error.value, tone: 'error' };
  switch (popupResult.value) {
    case 'cancelled':
      return { text: t('step-up-result-cancelled'), tone: 'quiet' };
    case 'wrong_account':
      return {
        text: email.value
          ? t('step-up-result-wrong-account', { email: email.value })
          : t('step-up-result-wrong-account-no-email'),
        tone: 'error',
      };
    case 'not_fresh':
      return { text: t('step-up-result-not-fresh'), tone: 'error' };
    case 'signed_out':
      return { text: t('step-up-result-signed-out'), tone: 'error' };
    case 'failed':
      return { text: t('step-up-result-failed'), tone: 'error' };
    default:
      return null;
  }
});

/**
 * True when the session is confirmed, sending a typed password or code first
 * if needed. False leaves the dialog open with the reason shown here.
 */
async function confirm(): Promise<boolean> {
  if (confirmed.value) return true;
  const value = credential.value;
  if (!typesCredential.value || value.trim().length === 0) return false;
  const ok = await stepUp.confirmWithCredential(
    method.value === 'password' ? { password: value } : { mfa_code: value.trim() },
  );
  if (ok) credential.value = '';
  return ok;
}

defineExpose({ confirm, refresh: stepUp.refresh });
</script>

<template>
  <div class="flex flex-col gap-3">
    <p v-if="confirmed" class="flex items-center gap-1.5 text-sm text-secondary">
      <Icon name="check" size="sm" class="text-status-success flex-shrink-0" />
      {{ $t('step-up-confirmed') }}
    </p>

    <template v-else>
      <p class="text-sm text-secondary">{{ $t('step-up-lead') }}</p>

      <p v-if="loadFailed" class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-status-error">
        {{ $t('step-up-result-failed') }}
        <button type="button" class="text-sm text-accent hover:underline" @click="stepUp.refresh()">
          {{ $t('step-up-try-again') }}
        </button>
      </p>

      <p v-else-if="!loading && method === null" class="text-sm text-secondary">
        {{ $t('step-up-no-methods', { minutes: windowMinutes }) }}
      </p>

      <PasswordInput
        v-else-if="method === 'password'"
        :id="fieldId"
        v-model="credential"
        :label="$t('step-up-password-label')"
        autocomplete="current-password"
        :disabled="busy"
        @keyup.enter="emit('submit')"
      />

      <FormInput
        v-else-if="method === 'code'"
        :id="fieldId"
        v-model="credential"
        :label="$t('step-up-code-label')"
        inputmode="numeric"
        autocomplete="one-time-code"
        :disabled="busy"
        @keyup.enter="emit('submit')"
      />

      <div v-else-if="method === 'passkey'" class="flex">
        <Button variant="secondary" icon="key" :loading="busy" @click="stepUp.confirmWithPasskey()">
          {{ $t('step-up-passkey-button') }}
        </Button>
      </div>

      <div v-else-if="method === 'provider'" class="flex flex-col gap-2">
        <template v-if="waiting">
          <p class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-secondary" role="status">
            {{ $t('step-up-waiting') }}
            <button type="button" class="text-sm text-accent hover:underline" @click="continueWithProvider">
              {{ $t('step-up-try-again') }}
            </button>
          </p>
        </template>
        <template v-else>
          <div class="flex">
            <Button
              variant="secondary"
              :icon="provider?.provider_type === 'microsoft' ? 'microsoft' : 'openExternal'"
              @click="continueWithProvider"
            >
              {{ $t('step-up-provider-button', { name: providerName }) }}
            </Button>
          </div>
          <p class="text-xs text-tertiary">{{ $t('step-up-provider-help') }}</p>
        </template>
      </div>

      <p
        v-if="message"
        class="text-sm"
        :class="message.tone === 'error' ? 'text-status-error' : 'text-secondary'"
        role="alert"
      >
        {{ message.text }}
      </p>

      <div v-if="otherMethods.length > 0" class="flex flex-col gap-1.5">
        <button
          v-if="!showOtherMethods"
          type="button"
          class="self-start text-sm text-accent hover:underline"
          :aria-expanded="false"
          @click="showOtherMethods = true"
        >
          {{ $t('step-up-use-another-way') }}
        </button>
        <ul v-else class="flex flex-col gap-1.5" :aria-label="$t('step-up-use-another-way')">
          <li v-for="m in otherMethods" :key="m">
            <button type="button" class="text-sm text-accent hover:underline text-left" @click="pickMethod(m)">
              {{ methodChoiceLabel(m) }}
            </button>
          </li>
        </ul>
      </div>
    </template>
  </div>
</template>
