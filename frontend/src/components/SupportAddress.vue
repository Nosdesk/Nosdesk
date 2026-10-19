<!--
  The Nosdesk support address on one line: a mailto link and a copy button,
  for people whose mail is in a browser tab rather than a mail app.
-->
<template>
  <span class="inline-flex items-center gap-1">
    <a :href="`mailto:${NOSDESK_SUPPORT_EMAIL}`" class="text-sm font-medium text-accent hover:underline">
      {{ NOSDESK_SUPPORT_EMAIL }}
    </a>
    <IconButton size="sm" icon="copy" :label="$t('support-address-copy')" @click="copyAddress" />
  </span>
</template>

<script setup lang="ts">
import { useFluent } from 'fluent-vue'
import IconButton from '@/components/common/IconButton.vue'
import { useClipboard } from '@/composables/useClipboard'
import { useToastStore } from '@nosdesk/core/stores/toast'
import { NOSDESK_SUPPORT_EMAIL } from '@/services/nosdeskAccount'

const fluent = useFluent()
const toast = useToastStore()
const { copy } = useClipboard()

async function copyAddress() {
  await copy(NOSDESK_SUPPORT_EMAIL)
  toast.success(fluent.$t('support-address-copied'))
}
</script>
