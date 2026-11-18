<script setup lang="ts">
/**
 * A plugin event trigger's settings: an installed plugin, and one of the
 * events its manifest declares (its own, `<plugin name>:<event>`). The rule runs on the ticket the event
 * names, when the plugin may read it. A plugin or event a saved rule names
 * that is gone stays listed, so the rule still reads.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';

import BaseDropdown from '@/components/common/BaseDropdown.vue';
import { useWorkspaceQuery } from '@/queries/workspaceQuery';
import pluginService from '@nosdesk/core/services/pluginService';
import type { Plugin, PluginState } from '@nosdesk/core/types/plugin';

const props = defineProps<{
  pluginUuid: string;
  eventType: string;
  disabled?: boolean;
  /** Set by a save attempt, so a missing pick shows its hint. */
  showProblems?: boolean;
}>();

const emit = defineEmits<{
  (e: 'update:pluginUuid', value: string): void;
  (e: 'update:eventType', value: string): void;
}>();

const { $t: t } = useFluent();

const pluginsQuery = useWorkspaceQuery({
  key: ['plugins', 'installed'],
  query: () => pluginService.listPlugins(),
  staleTime: 60 * 1000,
});
const plugins = computed<Plugin[]>(() => pluginsQuery.data.value ?? []);

/** Installed, if not necessarily running: an uninstalled or quarantined
 *  plugin emits nothing. */
const LISTED: PluginState[] = ['installed', 'disabled', 'awaiting_consent'];
const emitsOf = (plugin: Plugin | undefined): string[] => plugin?.manifest.events ?? [];
const selected = computed(() => plugins.value.find((p) => p.uuid === props.pluginUuid));

function pluginLabel(plugin: Plugin): string {
  const name = plugin.display_name || plugin.name;
  return plugin.state === 'installed' ? name : t('admin-rule-editor-plugin-not-running', { name });
}

const pluginOptions = computed(() => {
  const options = plugins.value
    .filter((p) => LISTED.includes(p.state) && emitsOf(p).length > 0)
    .map((p) => ({ value: p.uuid, label: pluginLabel(p) }));
  if (props.pluginUuid && !options.some((o) => o.value === props.pluginUuid)) {
    options.push({
      value: props.pluginUuid,
      label: selected.value ? pluginLabel(selected.value) : t('admin-rule-editor-plugin-missing'),
    });
  }
  return options;
});

const eventOptions = computed(() => {
  const events = [...emitsOf(selected.value)];
  if (props.eventType && !events.includes(props.eventType)) events.push(props.eventType);
  return events.map((e) => ({ value: e, label: e }));
});

/** A plugin's events are its own (`<plugin name>:<event>`), so another
 *  plugin takes its only event, or waits for a pick. */
function pickPlugin(uuid: string): void {
  if (uuid === props.pluginUuid) return;
  emit('update:pluginUuid', uuid);
  const events = emitsOf(plugins.value.find((p) => p.uuid === uuid));
  emit('update:eventType', events.length === 1 ? events[0] : '');
}

const noneToPick = computed(
  () => pluginsQuery.data.value !== undefined && pluginOptions.value.length === 0,
);
</script>

<template>
  <div class="flex flex-col gap-2" data-test="plugin-event">
    <p v-if="noneToPick" class="text-sm text-secondary" data-test="plugin-event-none">
      {{ t('admin-rule-editor-plugin-none') }}
    </p>
    <div v-else class="flex flex-col gap-2 sm:flex-row">
      <BaseDropdown
        class="flex-1 min-w-0"
        :model-value="pluginUuid"
        :options="pluginOptions"
        :label="t('admin-rule-editor-plugin-label')"
        :aria-label="t('admin-rule-editor-plugin-label')"
        :placeholder="t('admin-rule-editor-plugin-placeholder')"
        :disabled="disabled"
        size="sm"
        @update:model-value="pickPlugin(String($event))"
      />
      <BaseDropdown
        class="flex-1 min-w-0"
        :model-value="eventType"
        :options="eventOptions"
        :label="t('admin-rule-editor-plugin-event-label')"
        :aria-label="t('admin-rule-editor-plugin-event-label')"
        :placeholder="t('admin-rule-editor-plugin-event-placeholder')"
        :disabled="disabled || !pluginUuid"
        size="sm"
        @update:model-value="emit('update:eventType', String($event))"
      />
    </div>
    <p
      v-if="showProblems && (!pluginUuid || !eventType)"
      class="text-xs text-status-warning"
    >
      {{ t('admin-rule-editor-plugin-incomplete') }}
    </p>
    <p class="text-xs text-secondary">{{ t('admin-rule-editor-plugin-hint') }}</p>
  </div>
</template>
