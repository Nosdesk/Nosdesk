<script setup lang="ts">
/**
 * "Looking for your requests?": people who ask for help sign in at their
 * organisation's support portal, not here. Takes its address (or, on Nosdesk
 * Cloud, its short name) and opens it in the browser.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'
import { openExternalUrl } from '@/platform'
import { portalAddressFrom } from '@/utils/portalAddress'
import { getPortalDomain } from '@nosdesk/core/services/instanceConfig'

const { $t: t } = useFluent()
const open = ref(false)
const address = ref('')
const url = computed(() => portalAddressFrom(address.value, getPortalDomain()))
const invalid = computed(() => address.value.trim() !== '' && !url.value)

function go(): void {
  if (url.value) void openExternalUrl(url.value)
}
</script>

<template>
  <div class="flex flex-col gap-3 rounded-xl border border-default bg-surface px-4 py-3">
    <button
      type="button"
      class="self-start text-sm font-medium text-accent hover:underline"
      :aria-expanded="open"
      @click="open = !open"
    >
      {{ t('requester-portal-link-toggle') }}
    </button>
    <form v-if="open" class="flex flex-col gap-3" @submit.prevent="go">
      <p class="text-sm text-secondary">{{ t('requester-portal-link-help') }}</p>
      <FormInput
        v-model="address"
        size="sm"
        inputmode="url"
        autocapitalize="off"
        autocomplete="off"
        :label="t('requester-portal-link-label')"
        :placeholder="t('requester-portal-link-placeholder')"
        :error="invalid ? t('requester-portal-link-invalid') : undefined"
      />
      <Button type="submit" variant="secondary" :disabled="!url">{{ t('requester-portal-link-open') }}</Button>
    </form>
  </div>
</template>
