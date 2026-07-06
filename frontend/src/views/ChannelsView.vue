<script setup lang="ts">
/**
 * Channels — the list of inbound sources that turn messages into tickets.
 *
 * Channels are typed (`provider`); today the only type is `email_imap`, but the
 * list + "Add channel" type picker is the extensibility seam: a new channel type
 * is one entry in CHANNEL_TYPES + its config view, not a new bespoke admin page.
 */
import { ref, computed } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery } from '@pinia/colada';

import { channelsService, type Channel, type ImapRuntimeState } from '@nosdesk/core/services/channelsService';
import { isInboundForwardingEnabled } from '@nosdesk/core/services/instanceConfig';
import type { IconName } from '@/components/common/icons';
import Icon from '@/components/common/Icon.vue';
import Button from '@/components/common/Button.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);
const router = useRouter();

/** The channel-type registry. Add a type here + its config route to extend. */
interface ChannelType {
  provider: string;
  icon: IconName;
  titleKey: string;
  descriptionKey: string;
  route: string;
}
const CHANNEL_TYPES: ChannelType[] = [
  {
    provider: 'email_forward',
    icon: 'email',
    titleKey: 'admin-channels-type-email-forward',
    descriptionKey: 'admin-channels-type-email-forward-description',
    route: '/admin/channels/forwarding',
  },
  {
    provider: 'email_imap',
    icon: 'email',
    titleKey: 'admin-channels-type-email-imap',
    descriptionKey: 'admin-channels-type-email-imap-description',
    route: '/admin/channels/email',
  },
  {
    // Hosted managed default address (support@<slug>.<tenant domain>).
    // Created automatically by the platform when mail arrives — never
    // addable from the picker; the row links to the Email delivery page.
    provider: 'email_managed',
    icon: 'email',
    titleKey: 'admin-channels-type-email-managed',
    descriptionKey: 'admin-channels-type-email-managed-description',
    route: '/admin/email/delivery',
  },
];

const typeFor = (provider: string) => CHANNEL_TYPES.find((c) => c.provider === provider);

// The add-channel picker hides forwarding unless the instance can receive it
// (needs an inbound domain), and always hides the managed channel (the
// platform mints it; there is nothing to configure). `typeFor` still
// resolves both so existing channels render correctly regardless.
const addableTypes = computed(() =>
  CHANNEL_TYPES.filter(
    (c) =>
      c.provider !== 'email_managed' &&
      (c.provider !== 'email_forward' || isInboundForwardingEnabled()),
  ),
);

const CHANNELS_KEY = ['admin-channels-list'] as const;
const channelsQuery = useQuery({
  key: CHANNELS_KEY,
  query: () => channelsService.list(),
});
const channels = computed<Channel[]>(() => channelsQuery.data.value ?? []);
const isFirstLoad = computed(
  () => channelsQuery.status.value === 'pending' && channelsQuery.data.value === undefined,
);

const showAdd = ref(false);

function openChannel(ch: Channel) {
  const route = typeFor(ch.provider)?.route;
  if (route) router.push(route);
}

type Status = 'active' | 'disabled' | 'error';
function statusOf(ch: Channel): Status {
  if ((ch.runtime_state as ImapRuntimeState)?.last_error) return 'error';
  return ch.enabled ? 'active' : 'disabled';
}
const STATUS_CLASS: Record<Status, string> = {
  active: 'bg-status-success/20 text-status-success border-status-success/50',
  disabled: 'bg-surface-alt text-tertiary border-default',
  error: 'bg-status-error/20 text-status-error border-status-error/50',
};
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <header class="flex items-start justify-between gap-4">
        <div class="flex flex-col gap-1">
          <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ t('admin-channels-list-title') }}</h1>
          <p class="text-secondary">{{ t('admin-channels-list-description') }}</p>
        </div>
        <Button @click="showAdd = !showAdd">{{ t('admin-channels-add') }}</Button>
      </header>

      <!-- Add-channel type picker (the extensibility seam). -->
      <div v-if="showAdd" class="rounded-xl border border-default bg-surface p-4 flex flex-col gap-2">
        <p class="text-sm text-secondary">{{ t('admin-channels-add-prompt') }}</p>
        <button
          v-for="ct in addableTypes"
          :key="ct.provider"
          type="button"
          class="flex items-center gap-3 p-3 rounded-lg border border-default hover:border-strong text-left transition-colors"
          @click="router.push(ct.route)"
        >
          <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-accent/20 flex items-center justify-center text-accent">
            <Icon :name="ct.icon" size="md" />
          </div>
          <div class="flex flex-col">
            <span class="font-medium text-primary">{{ t(ct.titleKey) }}</span>
            <span class="text-xs text-tertiary">{{ t(ct.descriptionKey) }}</span>
          </div>
        </button>
      </div>

      <!-- Cold-cache skeleton only; revisits render instantly. -->
      <div v-if="isFirstLoad" class="flex flex-col gap-3">
        <Skeleton v-for="n in 2" :key="n" class="h-16 rounded-xl">
          <SkeletonBar class="w-40" />
        </Skeleton>
      </div>

      <!-- Channel list. -->
      <div v-else-if="channels.length" class="flex flex-col gap-3">
        <button
          v-for="ch in channels"
          :key="ch.id"
          type="button"
          class="flex items-center gap-3 p-4 rounded-xl border border-default bg-surface hover:border-strong text-left transition-colors"
          @click="openChannel(ch)"
        >
          <div class="flex-shrink-0 h-9 w-9 rounded-lg bg-accent/20 flex items-center justify-center text-accent">
            <Icon :name="typeFor(ch.provider)?.icon ?? 'email'" size="md" />
          </div>
          <div class="flex-1 flex flex-col">
            <span class="font-medium text-primary">{{ ch.name }}</span>
            <span class="text-xs text-tertiary">
              {{ typeFor(ch.provider) ? t(typeFor(ch.provider)!.titleKey) : ch.provider }}
              <template v-if="ch.managed_address"> · <span class="font-mono select-all">{{ ch.managed_address }}</span></template>
            </span>
          </div>
          <span class="px-1.5 py-0.5 text-xs rounded-full border" :class="STATUS_CLASS[statusOf(ch)]">
            {{ t(`admin-channels-status-${statusOf(ch)}`) }}
          </span>
        </button>
      </div>

      <!-- Empty state. -->
      <div v-else class="text-center py-12 bg-surface rounded-xl border border-default p-6 flex flex-col items-center gap-3">
        <Icon name="email" size="lg" class="text-tertiary" />
        <p class="text-lg font-medium text-primary">{{ t('admin-channels-empty-title') }}</p>
        <p class="text-tertiary">{{ t('admin-channels-empty-description') }}</p>
        <Button class="mt-2" @click="showAdd = true">{{ t('admin-channels-add') }}</Button>
      </div>
    </div>
  </div>
</template>
