<script setup lang="ts">
// The queue before the first ticket ever. One heading, the three ways a
// ticket arrives as links to those places, and "invite your team". It
// never shows again once the pool holds a ticket (docs/plans/first-run.md
// slice 3). Agents cannot connect a mailbox or share the form, so they
// see the heading and "Create one".
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { useClipboard } from '@/composables/useClipboard'
import { useToastStore } from '@nosdesk/core/stores/toast'
import { useWorkspacePortal } from '@/composables/useWorkspacePortal'

const props = defineProps<{
  /** Workspace admin: shows the mailbox, request form and invite links. */
  isAdmin: boolean
}>()
const emit = defineEmits<{ (e: 'create'): void }>()

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)
const toast = useToastStore()
const { copy } = useClipboard()

// The public request form lives on the workspace's portal, which on hosted is
// not the origin this page is served from.
const { portalUrl } = useWorkspacePortal()
const requestFormUrl = computed(() => portalUrl('/submit-ticket'))

async function copyRequestForm(): Promise<void> {
  await copy(requestFormUrl.value)
  toast.success(t('ticket-list-first-form-copied'))
}

const linkClass =
  'text-sm font-medium text-accent underline-offset-4 hover:underline focus-visible:underline'
</script>

<template>
  <div class="flex flex-col items-center gap-4 text-center">
    <div class="flex flex-col gap-1">
      <p class="font-medium text-primary">{{ t('ticket-list-first-title') }}</p>
      <p class="text-xs text-tertiary">
        {{ props.isAdmin ? t('ticket-list-first-body-admin') : t('ticket-list-first-body') }}
        <RouterLink
          v-if="props.isAdmin"
          to="/documentation/collections/getting-started"
          class="underline underline-offset-4 hover:text-secondary"
          >{{ t('ticket-list-first-guide') }}</RouterLink
        >
      </p>
    </div>
    <div class="flex flex-wrap items-center justify-center gap-x-6 gap-y-2">
      <RouterLink v-if="props.isAdmin" :to="{ name: 'admin-email-settings' }" :class="linkClass">
        {{ t('ticket-list-first-connect-mailbox') }}
      </RouterLink>
      <button v-if="props.isAdmin" type="button" :class="linkClass" @click="copyRequestForm">
        {{ t('ticket-list-first-share-form') }}
      </button>
      <button type="button" :class="linkClass" @click="emit('create')">
        {{ t('ticket-list-first-create') }}
      </button>
    </div>
    <RouterLink
      v-if="props.isAdmin"
      to="/users"
      class="text-xs text-tertiary underline-offset-4 hover:text-secondary hover:underline"
    >
      {{ t('ticket-list-first-invite') }}
    </RouterLink>
  </div>
</template>
