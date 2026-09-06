<!-- NoWorkspaceAccessView.vue
     Landing for an authenticated agent who belongs to no workspace yet.
     Under Model C the workspace is a post-login selection, so a user whose
     seat hasn't been provisioned (or was just revoked) has nothing to land
     on. The router sends them here instead of falling through to a 404. -->
<script setup lang="ts">
import { ref } from 'vue';
import { useAuthStore } from '@/stores/auth';
import AuthLayout from '@/components/auth/AuthLayout.vue';
import Button from '@/components/common/Button.vue';

const auth = useAuthStore();

const refreshing = ref(false);
const signingOut = ref(false);

// Full reload so the post-login landing guard re-runs against a fresh
// membership list and routes into a workspace if one was just granted.
function refresh() {
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
    <div class="flex flex-col gap-6">
      <div class="flex flex-col gap-2 text-center lg:text-left">
        <h1 class="text-2xl sm:text-3xl font-semibold tracking-tight text-primary">
          {{ $t('no-workspace-access-title') }}
        </h1>
        <p class="text-base text-secondary">{{ $t('no-workspace-access-message') }}</p>
        <!-- Post-auth, so naming the account is safe, and it makes "wrong
             account? sign out" actionable instead of a guessing game. -->
        <p v-if="auth.user?.email" class="text-sm text-tertiary">
          {{ $t('no-workspace-access-signed-in-as', { email: auth.user.email }) }}
        </p>
      </div>
      <p class="text-sm text-tertiary">{{ $t('no-workspace-access-description') }}</p>
      <div class="flex gap-3">
        <Button variant="primary" :loading="refreshing" @click="refresh">
          {{ $t('no-workspace-access-refresh') }}
        </Button>
        <Button variant="secondary" :loading="signingOut" @click="signOut">
          {{ $t('no-workspace-access-sign-out') }}
        </Button>
      </div>
    </div>
  </AuthLayout>
</template>
