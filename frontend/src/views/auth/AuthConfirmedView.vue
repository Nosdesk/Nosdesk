<!--
Where the "Confirm it's you" popup lands after the identity provider. It
tells the dialog that opened it how it went, over a BroadcastChannel (the
app sends COOP same-origin, so there is no `window.opener` to call), then
tries to close itself.

Public on purpose: the provider's redirect is a cross-site navigation, so
SameSite=Strict session cookies are not sent with it. Nothing here needs them.
-->
<script setup lang="ts">
import { computed, onMounted } from 'vue';
import { useRoute } from 'vue-router';
import { useFluent } from 'fluent-vue';
import Icon from '@/components/common/Icon.vue';
import {
  STEP_UP_CHANNEL,
  STEP_UP_POPUP_RESULTS,
  type StepUpChannelMessage,
  type StepUpPopupResult,
} from '@/services/stepUpService';

const route = useRoute();
const fluent = useFluent();

const result = computed<StepUpPopupResult>(() => {
  const raw = route.query.result;
  const value = Array.isArray(raw) ? raw[0] : raw;
  return STEP_UP_POPUP_RESULTS.find((r) => r === value) ?? 'failed';
});

const message = computed(() => {
  switch (result.value) {
    case 'ok':
      return fluent.$t('step-up-popup-confirmed');
    case 'cancelled':
      return fluent.$t('step-up-result-cancelled');
    case 'wrong_account':
      // The popup has no session to read the expected address from.
      return fluent.$t('step-up-result-wrong-account-no-email');
    case 'not_fresh':
      return fluent.$t('step-up-result-not-fresh');
    case 'signed_out':
      return fluent.$t('step-up-result-signed-out');
    default:
      return fluent.$t('step-up-result-failed');
  }
});

onMounted(() => {
  try {
    const channel = new BroadcastChannel(STEP_UP_CHANNEL);
    const payload: StepUpChannelMessage = { type: 'step-up', result: result.value };
    channel.postMessage(payload);
    channel.close();
  } catch {
    // No BroadcastChannel: the dialog still notices on focus or its next poll.
  }
  // Browsers only let a script close a window a script opened; otherwise the
  // page says it can be closed.
  try {
    window.close();
  } catch {
    // ignore
  }
});
</script>

<template>
  <div class="min-h-screen flex items-center justify-center bg-app p-4">
    <div
      class="bg-surface p-8 rounded-xl shadow-lg max-w-sm w-full border border-default flex flex-col items-center gap-4 text-center"
    >
      <div
        class="w-12 h-12 rounded-full flex items-center justify-center"
        :class="result === 'ok' ? 'bg-status-success/10 text-status-success' : 'bg-status-warning/10 text-status-warning'"
      >
        <Icon :name="result === 'ok' ? 'check' : 'warning'" size="lg" />
      </div>
      <p class="text-base font-medium text-primary" role="status">{{ message }}</p>
      <p v-if="result !== 'ok'" class="text-sm text-secondary">{{ $t('step-up-popup-close') }}</p>
    </div>
  </div>
</template>
