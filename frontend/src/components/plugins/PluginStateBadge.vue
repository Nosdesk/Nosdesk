<script setup lang="ts">
/**
 * Renders a tier-distinct pill for the plugin lifecycle state.
 * Drives the row badge AND the read-only label that replaces the
 * toggle for non-toggleable states. Centralised here so the list
 * view, detail view, and registry view can't drift.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import type { PluginState } from '@nosdesk/core/types/plugin';

interface Props {
  state: PluginState;
}

const props = defineProps<Props>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const STYLES: Record<PluginState, { pillClass: string; textClass: string }> = {
  installed: {
    pillClass: 'bg-status-success/10 text-status-success',
    textClass: 'text-status-success',
  },
  disabled: {
    pillClass: 'bg-status-warning/10 text-status-warning',
    textClass: 'text-status-warning',
  },
  quarantined: {
    pillClass: 'bg-status-error/10 text-status-error',
    textClass: 'text-status-error',
  },
  uninstalled: {
    pillClass: 'bg-surface-alt text-tertiary',
    textClass: 'text-tertiary',
  },
};

const LABEL_KEY: Record<PluginState, string> = {
  installed: 'plugin-state-active',
  disabled: 'plugin-state-disabled',
  quarantined: 'plugin-state-quarantined',
  uninstalled: 'plugin-state-uninstalled',
};

const label = computed(() => t(LABEL_KEY[props.state]));
const pillClass = computed(() => STYLES[props.state].pillClass);
</script>

<template>
  <span
    class="inline-flex items-center rounded px-1.5 py-0.5 text-xs font-medium"
    :class="pillClass"
  >
    {{ label }}
  </span>
</template>
