<script setup lang="ts">
/**
 * Done's sign-in line for a users import: how many of the people it
 * created can't sign in yet, and how they will. Nothing was sent by the
 * import. Where an invitation sets a password, "Send invites" sends one to
 * each person without an open invitation; under the workspace's sign-in
 * provider, the line says they sign in with it.
 */
import { computed, ref } from 'vue'

import Button from '@/components/common/Button.vue'
import { importService, type InvitesResult, type SignInStatus } from '@nosdesk/core/services/importService'

const props = defineProps<{
  jobId: string
  status: SignInStatus
  /** Sends the invites; a users import's by default. A workspace import
   *  passes its own. */
  sendInvites?: () => Promise<InvitesResult>
}>()

const emit = defineEmits<{
  /** Send invites ran: the line as it stands now (null when nobody is
   *  waiting any more). */
  (e: 'updated', status: SignInStatus | null): void
}>()

const sending = ref(false)
/** Invites that weren't sent on the last press, or -1 when it failed. */
const failed = ref(0)

/** Everyone waiting holds an open invitation. */
const allInvited = computed(() => props.status.invited >= props.status.pending)

async function send() {
  if (sending.value) return
  sending.value = true
  failed.value = 0
  try {
    const result = props.sendInvites ? await props.sendInvites() : await importService.sendInvites(props.jobId)
    failed.value = result.failed
    emit('updated', result.sign_in)
  } catch {
    failed.value = -1
  } finally {
    sending.value = false
  }
}
</script>

<template>
  <div
    class="rounded-xl border border-default bg-surface px-4 py-3 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between"
    data-test="import-sign-in"
  >
    <div class="flex flex-col gap-1 min-w-0">
      <p class="text-sm text-primary font-medium" data-test="import-sign-in-pending">
        {{ $t('import-done-sign-in-pending', { count: status.pending }) }}
      </p>
      <p v-if="status.method === 'sso'" class="text-sm text-secondary" data-test="import-sign-in-sso">
        {{ $t('import-done-sign-in-sso', { provider: status.provider ?? '' }) }}
      </p>
      <p v-else-if="allInvited" class="text-sm text-secondary" data-test="import-sign-in-sent">
        {{ $t('import-done-invites-sent', { count: status.invited }) }}
      </p>
      <p v-else-if="!status.email_ready" class="text-sm text-secondary" data-test="import-sign-in-email-off">
        {{ $t('import-done-invites-email-off') }}
      </p>
      <p v-if="failed > 0" class="text-sm text-status-error" role="alert" data-test="import-sign-in-failed">
        {{ $t('import-done-invites-failed', { count: failed }) }}
      </p>
      <p v-else-if="failed < 0" class="text-sm text-status-error" role="alert" data-test="import-sign-in-failed">
        {{ $t('import-done-invites-error') }}
      </p>
    </div>
    <div v-if="status.method === 'invite' && !allInvited && status.email_ready" class="shrink-0">
      <Button variant="secondary" :loading="sending" data-test="import-sign-in-send" @click="send">
        {{ $t('import-done-send-invites', { count: status.pending - status.invited }) }}
      </Button>
    </div>
  </div>
</template>
