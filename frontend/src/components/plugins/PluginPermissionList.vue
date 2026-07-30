<script setup lang="ts">
/**
 * Renders a plugin's requested permissions as a human-readable list, with
 * destructive scopes (write/delete) flagged prominently. Shared by the consent
 * screen and the plugin detail page so the two never drift.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import { describePermission } from '@nosdesk/core/types/plugin';
import type { PluginPermission } from '@nosdesk/core/types/plugin';

const props = defineProps<{
  permissions: (PluginPermission | string)[];
  /** Author-supplied justifications keyed by permission string. Untrusted
   *  display text: rendered via text interpolation (escaped), never a grant. */
  reasons?: Record<string, string>;
}>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const items = computed(() =>
  props.permissions.map((value) => ({
    value,
    ...describePermission(value),
    reason: props.reasons?.[value]?.trim() || null,
  })),
);
</script>

<template>
  <ul v-if="items.length" class="flex flex-col gap-2">
    <li
      v-for="perm in items"
      :key="perm.value"
      class="flex items-start gap-2 rounded-md border p-2"
      :class="perm.destructive ? 'border-status-error/40 bg-status-error/5' : 'border-default'"
    >
      <div class="flex-1">
        <div class="flex items-center gap-2">
          <span class="text-sm font-medium text-primary">{{ t(perm.labelKey, perm.args) }}</span>
          <span
            v-if="perm.destructive"
            class="rounded bg-status-error/15 px-1.5 py-0.5 text-xs font-medium text-status-error"
          >
            {{ t('plugin-permission-destructive-badge') }}
          </span>
        </div>
        <p class="mt-0.5 text-xs text-secondary">{{ t(perm.descriptionKey, perm.args) }}</p>
        <p
          v-if="perm.reason"
          class="mt-1 border-l-2 border-default pl-2 text-xs italic text-tertiary"
        >
          {{ t('plugin-permission-reason-prefix') }} {{ perm.reason }}
        </p>
      </div>
    </li>
  </ul>
</template>
