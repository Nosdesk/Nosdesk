<!-- NoWorkspaceAccessView.vue
     Landing for an authenticated agent who belongs to no workspace yet.
     Under Model C the workspace is a post-login selection, so a user whose
     seat hasn't been provisioned (or was just revoked) has nothing to land
     on. The router sends them here instead of falling through to a 404.

     The page's job is to make the two likely mistakes visible at a glance:
     the wrong account, or the wrong server. So the account and the server
     are the one prominent element; the copy around them is the fix. -->
<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { useAuthStore } from '@/stores/auth';
import { isTauriRuntime } from '@/platform';
import AuthLayout from '@/components/auth/AuthLayout.vue';
import Button from '@/components/common/Button.vue';
import Icon from '@/components/common/Icon.vue';
import UserAvatar from '@/components/UserAvatar.vue';

const auth = useAuthStore();

const refreshing = ref(false);
const signingOut = ref(false);

// The host this session is signed in to: the stored server in the native
// app (where a wrong pick is the common cause), the page origin on the web.
const serverHost = ref(window.location.host);
onMounted(async () => {
  if (!isTauriRuntime()) return;
  const { getStoredServer } = await import('@nosdesk/mobile');
  const stored = getStoredServer();
  if (stored) serverHost.value = new URL(stored).host;
});

// Full reload so the post-login landing guard re-runs against a fresh
// membership list and routes into a workspace if one was just granted.
function checkAgain() {
  refreshing.value = true;
  window.location.assign('/');
}

async function signOut() {
  signingOut.value = true;
  try {
    await auth.logout();
  } finally {
    signingOut.value = false;
  }
}
</script>

<template>
  <AuthLayout>
    <div class="flex flex-col gap-8">
      <div class="flex flex-col gap-2 text-center lg:text-left">
        <h1 class="text-2xl sm:text-3xl font-semibold tracking-tight text-primary">
          {{ $t('no-workspace-access-title') }}
        </h1>
        <p class="text-base text-secondary text-pretty">{{ $t('no-workspace-access-message') }}</p>
      </div>

      <!-- Who and where. Post-auth, so naming the account is safe, and it
           turns "wrong account? wrong server?" into a glance instead of a
           guess. -->
      <dl class="divide-y divide-default rounded-xl border border-default bg-surface">
        <div v-if="auth.user?.email" class="flex items-center gap-3 px-4 py-3">
          <UserAvatar
            :fallback-name="auth.user.name || auth.user.email"
            :fallback-avatar="auth.user.avatar_thumb || auth.user.avatar_url || null"
            :show-name="false"
            :clickable="false"
            size="sm"
          />
          <div class="min-w-0 flex-1">
            <dd class="truncate text-sm font-medium text-primary">{{ auth.user.email }}</dd>
            <dt class="text-xs text-tertiary">{{ $t('no-workspace-access-account-label') }}</dt>
          </div>
        </div>
        <div class="flex items-center gap-3 px-4 py-3">
          <span
            class="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-full bg-surface-alt text-secondary"
            aria-hidden="true"
          >
            <Icon name="server" size="sm" />
          </span>
          <div class="min-w-0 flex-1">
            <dd class="truncate text-sm font-medium text-primary">{{ serverHost }}</dd>
            <dt class="text-xs text-tertiary">{{ $t('no-workspace-access-server-label') }}</dt>
          </div>
        </div>
      </dl>

      <!-- text-pretty keeps a lone word off the last line, whatever the width or locale. -->
      <p class="text-sm leading-relaxed text-secondary text-center lg:text-left text-pretty">
        {{ $t('no-workspace-access-description') }}
      </p>

      <div class="flex flex-col items-center gap-4 lg:items-start">
        <Button
          variant="primary"
          size="lg"
          class="w-full lg:w-auto"
          :loading="refreshing"
          @click="checkAgain"
        >
          {{ $t('no-workspace-access-check-again') }}
        </Button>
        <p class="text-sm text-tertiary">
          {{ $t('no-workspace-access-wrong-account') }}
          <button
            type="button"
            class="font-medium text-secondary underline underline-offset-2 hover:text-primary disabled:opacity-60 pointer-coarse:min-h-11"
            :disabled="signingOut"
            @click="signOut"
          >
            {{ $t('no-workspace-access-sign-out') }}
          </button>
        </p>
      </div>
    </div>
  </AuthLayout>
</template>
