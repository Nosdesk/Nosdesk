<script setup lang="ts">
/** Primary + directory-synced pills shared by the contact cards. */
import { useFluent } from 'fluent-vue';
defineProps<{ primary?: boolean; primaryLabel: string; synced?: boolean }>();
const fluent = useFluent();
const t = (key: string) => fluent.$t(key);
</script>

<template>
  <span
    v-if="primary"
    class="px-2 py-0.5 rounded-full text-xs font-medium bg-accent/20 text-accent shrink-0"
  >{{ primaryLabel }}</span>
  <span
    v-if="synced"
    class="px-2 py-0.5 rounded-full text-xs bg-surface-hover text-tertiary shrink-0"
  >{{ t('user-contact-synced-badge') }}</span>
</template>
