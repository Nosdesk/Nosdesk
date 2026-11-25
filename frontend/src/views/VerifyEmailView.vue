<script setup lang="ts">
// Redeems an address-confirmation link.
//
// Unauthenticated by design: the link is opened from a mail client, which may
// not be the browser holding the session, and requiring a login first would
// strand anyone confirming a second address from a different device. The token
// is the proof.
import { ref, onMounted } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import userService from '@/services/userService';
import { extractErrorMessage } from '@/utils/errors';
import Button from '@/components/common/Button.vue';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';

const route = useRoute();
const router = useRouter();
const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

type State = 'working' | 'verified' | 'failed';
const state = ref<State>('working');
const address = ref('');
const errorMessage = ref('');

onMounted(async () => {
  const token = route.query.token;
  if (typeof token !== 'string' || !token) {
    state.value = 'failed';
    errorMessage.value = t('verify-email-missing-token');
    return;
  }
  try {
    const result = await userService.verifyEmailToken(token);
    address.value = result.email ?? '';
    state.value = 'verified';
  } catch (error) {
    // The server answers the same way for expired, already-used and unknown
    // tokens, so the copy here stays equally undifferentiated.
    errorMessage.value = extractErrorMessage(error, t('verify-email-failed-body'));
    state.value = 'failed';
  }
});
</script>

<template>
  <div class="min-h-screen flex items-center justify-center px-4">
    <div class="w-full max-w-md flex flex-col items-center gap-4 text-center">
      <template v-if="state === 'working'">
        <Spinner size="lg" class="text-accent" />
        <p class="text-secondary text-sm">{{ $t('verify-email-working') }}</p>
      </template>

      <template v-else-if="state === 'verified'">
        <div class="w-12 h-12 rounded-full bg-status-success/10 flex items-center justify-center">
          <Icon name="check" class="h-6 w-6 text-status-success-ui" />
        </div>
        <h1 class="text-xl font-semibold text-primary">{{ $t('verify-email-success-title') }}</h1>
        <p class="text-secondary text-sm">
          {{ address ? t('verify-email-success-body-address', { address }) : $t('verify-email-success-body') }}
        </p>
        <Button variant="primary" @click="router.push('/')">
          {{ $t('verify-email-continue') }}
        </Button>
      </template>

      <template v-else>
        <div class="w-12 h-12 rounded-full bg-status-error/10 flex items-center justify-center">
          <Icon name="warning" class="h-6 w-6 text-status-error-ui" />
        </div>
        <h1 class="text-xl font-semibold text-primary">{{ $t('verify-email-failed-title') }}</h1>
        <p class="text-secondary text-sm">{{ errorMessage }}</p>
        <p class="text-tertiary text-xs">{{ $t('verify-email-failed-hint') }}</p>
        <Button variant="secondary" @click="router.push('/')">
          {{ $t('verify-email-continue') }}
        </Button>
      </template>
    </div>
  </div>
</template>
