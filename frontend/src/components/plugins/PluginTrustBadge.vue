<script setup lang="ts">
/**
 * Trust-tier pill: official / verified / community / local.
 * Mirrors the backend's resolved tier vocabulary; one component
 * everywhere this needs to render.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import type { PluginTrustLevel } from '@nosdesk/core/types/plugin';

interface Props {
  level: PluginTrustLevel;
}

const props = defineProps<Props>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const PILL_CLASS: Record<PluginTrustLevel, string> = {
  official: 'bg-status-success/10 text-status-success',
  verified: 'bg-accent/10 text-accent',
  community: 'bg-surface-alt text-secondary',
  local: 'bg-status-info/10 text-status-info',
};

const LABEL_KEY: Record<PluginTrustLevel, string> = {
  official: 'plugin-trust-official',
  verified: 'plugin-trust-verified',
  community: 'plugin-trust-community',
  local: 'plugin-trust-local',
};

const label = computed(() => t(LABEL_KEY[props.level]));
const pillClass = computed(() => PILL_CLASS[props.level]);
</script>

<template>
  <span
    class="inline-flex items-center rounded px-1.5 py-0.5 text-xs font-medium"
    :class="pillClass"
  >
    {{ label }}
  </span>
</template>
