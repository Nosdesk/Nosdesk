<script setup lang="ts">
/**
 * Trust-tier pill: official / verified / community / local.
 * Mirrors the backend's resolved tier vocabulary; one component
 * everywhere this needs to render.
 */
import type { PluginTrustLevel } from '@/types/plugin';

interface Props {
  level: PluginTrustLevel;
}

defineProps<Props>();

const META: Record<PluginTrustLevel, { label: string; pillClass: string }> = {
  official: { label: 'Official', pillClass: 'bg-status-success/10 text-status-success' },
  verified: { label: 'Verified', pillClass: 'bg-accent/10 text-accent' },
  community: { label: 'Community', pillClass: 'bg-surface-alt text-secondary' },
  local: { label: 'Local', pillClass: 'bg-status-info/10 text-status-info' },
};
</script>

<template>
  <span
    class="inline-flex items-center rounded px-1.5 py-0.5 text-xs font-medium"
    :class="META[level].pillClass"
  >
    {{ META[level].label }}
  </span>
</template>
