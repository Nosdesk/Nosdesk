<script setup lang="ts">
/**
 * Config view for the `email_forward` channel.
 *
 * Unlike the IMAP channel there's nothing to configure: the backend mints a
 * `<token>@<inbound_domain>` address, and the admin's only job is to forward
 * their support mailbox to it. So this view either offers to create the
 * channel (one click) or shows the generated address + copy-paste forwarding
 * instructions for the existing one.
 */
import { ref, computed } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';

import { channelsService, type Channel } from '@/services/channelsService';
import { isInboundForwardingEnabled } from '@/services/instanceConfig';
import Button from '@/components/common/Button.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

const forwardingEnabled = isInboundForwardingEnabled();

// Share the channel list cache with ChannelsView so navigating here renders
// instantly and a create here updates the list there.
const CHANNELS_KEY = ['admin-channels-list'] as const;
const queryCache = useQueryCache();
const channelsQuery = useQuery({
  key: CHANNELS_KEY,
  query: () => channelsService.list(),
});
const forwardChannel = computed<Channel | undefined>(() =>
  channelsQuery.data.value?.find((c) => c.provider === 'email_forward'),
);

const name = ref('');
const isCreating = ref(false);
const errorMessage = ref('');

async function createChannel() {
  if (isCreating.value) return;
  isCreating.value = true;
  errorMessage.value = '';
  try {
    await channelsService.createForwarding(name.value.trim() || t('admin-channels-forwarding-default-name'));
    await queryCache.invalidateQueries({ key: CHANNELS_KEY });
  } catch (err) {
    const e = err as { response?: { data?: { error?: string } }; message?: string };
    errorMessage.value = e.response?.data?.error || e.message || t('admin-channels-forwarding-error-create');
  } finally {
    isCreating.value = false;
  }
}

const copied = ref(false);
async function copyAddress() {
  const address = forwardChannel.value?.forwarding_address;
  if (!address) return;
  try {
    await navigator.clipboard.writeText(address);
    copied.value = true;
    setTimeout(() => (copied.value = false), 2000);
  } catch {
    // Clipboard blocked (insecure context / permissions); the address is
    // visible and selectable anyway, so there's nothing to recover.
  }
}
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-6xl">
      <RouterLink :to="{ name: 'admin-channels' }" class="text-sm text-secondary hover:text-primary w-fit">
        &lsaquo; {{ t('admin-nav-channels-title') }}
      </RouterLink>
      <div class="flex flex-col gap-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ t('admin-channels-forwarding-title') }}</h1>
        <p class="text-secondary">{{ t('admin-channels-forwarding-description') }}</p>
      </div>

      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Instance can't receive forwarded mail (self-host / not configured). -->
      <div
        v-if="!forwardingEnabled"
        class="bg-surface border border-default rounded-xl p-6 text-sm text-secondary"
      >
        {{ t('admin-channels-forwarding-not-enabled') }}
      </div>

      <!-- Existing channel: show its address + forwarding instructions. -->
      <div
        v-else-if="forwardChannel?.forwarding_address"
        class="flex flex-col gap-6"
      >
        <div class="bg-surface border border-default rounded-xl p-6 flex flex-col gap-4">
          <div class="flex flex-col gap-1">
            <h2 class="text-lg font-semibold text-primary">{{ t('admin-channels-forwarding-address-heading') }}</h2>
            <p class="text-sm text-secondary">{{ t('admin-channels-forwarding-address-description') }}</p>
          </div>
          <div class="flex items-center gap-3 flex-wrap">
            <code class="flex-1 min-w-0 truncate text-sm font-mono bg-input border border-default rounded px-3 py-2 text-primary">
              {{ forwardChannel.forwarding_address }}
            </code>
            <Button variant="secondary" @click="copyAddress">
              {{ copied ? t('admin-channels-forwarding-copied') : t('admin-channels-forwarding-copy') }}
            </Button>
          </div>
        </div>

        <div class="bg-surface border border-default rounded-xl p-6 flex flex-col gap-2">
          <h2 class="text-lg font-semibold text-primary">{{ t('admin-channels-forwarding-instructions-heading') }}</h2>
          <p class="text-sm text-secondary">{{ t('admin-channels-forwarding-instructions-body') }}</p>
        </div>
      </div>

      <!-- No channel yet: one-click create. -->
      <div
        v-else
        class="bg-surface border border-default rounded-xl p-6 flex flex-col gap-4"
      >
        <div class="flex flex-col gap-1">
          <h2 class="text-lg font-semibold text-primary">{{ t('admin-channels-forwarding-create-heading') }}</h2>
          <p class="text-sm text-secondary">{{ t('admin-channels-forwarding-create-description') }}</p>
        </div>
        <input
          v-model="name"
          type="text"
          :placeholder="t('admin-channels-forwarding-name-placeholder')"
          class="h-9 px-2 rounded border border-default bg-input text-primary text-sm max-w-sm"
        />
        <Button class="w-fit" :disabled="isCreating" @click="createChannel">
          {{ isCreating ? t('admin-channels-forwarding-creating') : t('admin-channels-forwarding-create-button') }}
        </Button>
      </div>
    </div>
  </div>
</template>
