<script setup lang="ts">
/** Marks a person whose identity their Nosdesk account owns (hosted staff). */
import { useFluent } from 'fluent-vue';

import Icon from '@/components/common/Icon.vue';

const { $t: t } = useFluent();
</script>

<template>
  <span
    class="inline-flex items-center gap-1 text-2xs font-medium px-1.5 py-0.5 rounded-md bg-surface-alt border border-default text-secondary whitespace-nowrap"
    :title="t('nosdesk-account-chip-tooltip')"
  >
    <Icon name="lock" size="xs" />
    {{ t('nosdesk-account-chip') }}
  </span>
</template>
