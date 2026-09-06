<!--
First-run "choose your Nosdesk server" screen for the native (Tauri) app.

Nosdesk is self-hostable, so the app isn't pinned to the cloud: the user either
taps "Nosdesk Cloud" or enters their own instance's URL. The choice is validated
(HTTPS + confirmed to be a Nosdesk server) and persisted; the app then proceeds
to the normal login. Shown only in Tauri when no server is stored yet (see
platform/serverGate). On the web this never renders.
-->
<script setup lang="ts">
import { ref } from 'vue';
import AuthLayout from '@/components/auth/AuthLayout.vue';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';
import { selectCloud, connectTo } from '@/platform/serverGate';

/**
 * Where someone without an account goes. Points at the marketing site rather
 * than a trial-signup route until that flow exists; the landing page can route
 * them onward, so this needs no app release when it ships.
 */
const SIGN_UP_URL = 'https://nosdesk.com';

async function openSignUp() {
  const { openInBrowser } = await import('@nosdesk/mobile');
  await openInBrowser(SIGN_UP_URL);
}

const mode = ref<'choose' | 'self-hosted'>('choose');
const serverUrl = ref('');
const error = ref('');
const busy = ref(false);

async function chooseCloud() {
  busy.value = true;
  error.value = '';
  try {
    const result = await selectCloud();
    if (!result.ok) error.value = result.error ?? 'Could not connect to Nosdesk Cloud';
  } catch {
    error.value = 'Could not connect to Nosdesk Cloud';
  } finally {
    busy.value = false;
  }
}

async function connectSelfHosted() {
  if (!serverUrl.value.trim()) return;
  busy.value = true;
  error.value = '';
  try {
    const result = await connectTo(serverUrl.value);
    if (!result.ok) error.value = result.error ?? 'Could not connect';
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <AuthLayout>
    <template #pill>{{ $t('connect-title') }}</template>
    <template #hero-title>{{ $t('connect-hero-title') }}</template>
    <template #hero-subtitle>{{ $t('connect-hero-subtitle') }}</template>

    <div class="flex flex-col gap-8">
      <div>
        <h1 class="text-2xl font-semibold text-primary">{{ $t('connect-title') }}</h1>
        <p class="mt-2 text-sm text-secondary">{{ $t('connect-subtitle') }}</p>
      </div>

      <div
        v-if="error"
        class="flex items-center gap-2 rounded-lg border border-status-error/50 bg-status-error/10 px-4 py-3 text-sm text-status-error"
      >
        {{ error }}
      </div>

      <!-- Choose: cloud or self-hosted -->
      <div v-if="mode === 'choose'" class="flex flex-col gap-4">
        <Button variant="primary" block :loading="busy" @click="chooseCloud">
          {{ $t('connect-cloud') }}
        </Button>
        <button
          type="button"
          class="inline-flex w-full items-center justify-center text-sm font-medium text-accent hover:underline disabled:opacity-50 pointer-coarse:min-h-11"
          :disabled="busy"
          @click="mode = 'self-hosted'"
        >
          {{ $t('connect-self-hosted') }}
        </button>

        <!-- Someone with no account at all has nothing to type into either
             option above, and the app cannot create one for them. Hand them
             off to the website rather than leaving the screen a dead end. -->
        <div class="flex flex-col items-center gap-1">
          <p class="text-sm text-secondary">{{ $t('connect-no-account') }}</p>
          <button
            type="button"
            class="inline-flex items-center justify-center px-2 text-sm font-medium text-accent hover:underline pointer-coarse:min-h-11"
            @click="openSignUp"
          >
            {{ $t('connect-no-account-cta') }}
          </button>
        </div>
      </div>

      <!-- Self-hosted: enter a server URL -->
      <form v-else class="flex flex-col gap-6" @submit.prevent="connectSelfHosted">
        <FormInput
          v-model="serverUrl"
          type="text"
          inputmode="url"
          autocapitalize="none"
          autocorrect="off"
          spellcheck="false"
          :label="$t('connect-server-label')"
          :placeholder="$t('connect-server-placeholder')"
          :disabled="busy"
        />
        <Button type="submit" variant="primary" block :loading="busy" :disabled="!serverUrl.trim()">
          {{ $t('connect-submit') }}
        </Button>
        <button
          type="button"
          class="inline-flex w-full items-center justify-center text-sm font-medium text-secondary hover:underline disabled:opacity-50 pointer-coarse:min-h-11"
          :disabled="busy"
          @click="mode = 'choose'; error = ''"
        >
          {{ $t('connect-back') }}
        </button>
      </form>
    </div>
  </AuthLayout>
</template>
