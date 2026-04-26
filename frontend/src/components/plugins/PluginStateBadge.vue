<script setup lang="ts">
/**
 * Renders a tier-distinct pill for the plugin lifecycle state.
 * Drives the row badge AND the read-only label that replaces the
 * toggle for non-toggleable states. Centralised here so the list
 * view, detail view, and registry view can't drift.
 */
import type { PluginState } from '@/types/plugin';

interface Props {
  state: PluginState;
}

defineProps<Props>();

const META: Record<PluginState, { label: string; pillClass: string; textClass: string }> = {
  installed: {
    label: 'Active',
    pillClass: 'bg-status-success/10 text-status-success',
    textClass: 'text-status-success',
  },
  disabled: {
    label: 'Disabled',
    pillClass: 'bg-status-warning/10 text-status-warning',
    textClass: 'text-status-warning',
  },
  quarantined: {
    label: 'Quarantined',
    pillClass: 'bg-status-error/10 text-status-error',
    textClass: 'text-status-error',
  },
  uninstalled: {
    label: 'Uninstalled',
    pillClass: 'bg-surface-alt text-tertiary',
    textClass: 'text-tertiary',
  },
};
</script>

<template>
  <span
    class="inline-flex items-center rounded px-1.5 py-0.5 text-xs font-medium"
    :class="META[state].pillClass"
  >
    {{ META[state].label }}
  </span>
</template>
