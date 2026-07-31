<script setup lang="ts">
/**
 * Lifecycle-state pill for a plugin. A thin wrapper over the shared
 * StatusPill so plugin badges use the same tone palette as every other
 * status in the app. Centralised here so the list, detail, and registry
 * views can't drift.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import type { PluginState } from '@nosdesk/core/types/plugin';
import StatusPill from '@/components/common/StatusPill.vue';
import type { StatusPillTone } from '@/components/common/statusPillTone';

const props = defineProps<{ state: PluginState }>();

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

const TONE: Record<PluginState, StatusPillTone> = {
  installed: 'positive',
  awaiting_consent: 'caution',
  disabled: 'neutral',
  quarantined: 'critical',
  uninstalled: 'neutral',
};

const LABEL_KEY: Record<PluginState, string> = {
  installed: 'plugin-state-active',
  disabled: 'plugin-state-disabled',
  quarantined: 'plugin-state-quarantined',
  uninstalled: 'plugin-state-uninstalled',
  awaiting_consent: 'plugin-state-awaiting-consent',
};

const label = computed(() => t(LABEL_KEY[props.state]));
const tone = computed(() => TONE[props.state]);
</script>

<template>
  <StatusPill :tone="tone" :label="label" />
</template>
