<script setup lang="ts">
/**
 * The read-only stand-in for a person's identity (name, avatar, email, sign-in)
 * when their Nosdesk account owns it (hosted staff). One card instead of locked
 * fields scattered through the settings, with the one place to change them:
 * your own account settings, or (for workspace admins) the person's seat.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';

import Button from '@/components/common/Button.vue';
import Icon from '@/components/common/Icon.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import { openExternalUrl } from '@/platform';
import { controlPlaneSeatsUrl } from '@/services/activeWorkspace';
import { useAuthStore } from '@/stores/auth';
import { getAccountUrl } from '@nosdesk/core/services/instanceConfig';
import type { User } from '@nosdesk/core/types/user';

const props = defineProps<{
  user: Pick<User, 'uuid' | 'name' | 'email'>;
  /** What this card stands in for, which picks the body copy. */
  scope?: 'profile' | 'security';
}>();

const { $t: t } = useFluent();
const authStore = useAuthStore();

const isSelf = computed(() => authStore.user?.uuid === props.user.uuid);
const link = computed(() => {
  if (isSelf.value) return getAccountUrl();
  return authStore.isAdmin ? controlPlaneSeatsUrl(props.user.email) : '';
});
const body = computed(() => {
  const who = isSelf.value ? 'self' : 'other';
  return props.scope === 'security'
    ? t(`nosdesk-account-security-${who}`)
    : t(`nosdesk-account-profile-${who}`);
});
</script>

<template>
  <SectionCard content-padding="p-4 sm:p-6">
    <template #leading>
      <span class="text-accent inline-flex"><Icon name="lock" /></span>
    </template>
    <template #title>{{ t('nosdesk-account-title') }}</template>
    <div class="flex flex-col gap-4">
      <p class="text-sm text-secondary">{{ body }}</p>
      <dl v-if="scope !== 'security'" class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
        <dt class="text-tertiary">{{ t('nosdesk-account-name') }}</dt>
        <dd class="text-primary truncate">{{ user.name }}</dd>
        <template v-if="user.email">
          <dt class="text-tertiary">{{ t('nosdesk-account-email') }}</dt>
          <dd class="text-primary truncate">{{ user.email }}</dd>
        </template>
      </dl>
      <Button
        v-if="link"
        variant="secondary"
        icon="openExternal"
        class="self-start"
        @click="openExternalUrl(link)"
      >
        {{ isSelf ? t('nosdesk-account-open-self') : t('nosdesk-account-open-other') }}
      </Button>
    </div>
  </SectionCard>
</template>
