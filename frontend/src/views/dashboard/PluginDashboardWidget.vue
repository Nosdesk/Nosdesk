<!--
PluginDashboardWidget — the shell every plugin-contributed dashboard widget
renders through. The layout references a plugin widget via the synthetic id
`plugin_widget:<uuid>:<component>`; the widget registry resolves that prefix to
this component and threads the plugin uuid + component name through.

Plugin widgets are opt-in (added via the Add widget picker) and titled with the
plugin's manifest component label. If the plugin is no longer loaded (disabled
or uninstalled while still pinned to a layout), the shell shows an unavailable
state rather than a blank card.
-->
<script setup lang="ts">
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import DashboardWidgetShell from './DashboardWidgetShell.vue';
import PluginSandboxFrame from '@/plugins/components/PluginSandboxFrame.vue';
import { getLoadedPlugin, getSlotRegistrations } from '@/plugins/loader';

const props = defineProps<{
  pluginUuid: string;
  componentName: string;
}>();

const fluent = useFluent();
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args);

const loaded = computed(() => getLoadedPlugin(props.pluginUuid));

const registration = computed(() =>
  getSlotRegistrations('dashboard.widget').find(
    (r) => r.pluginUuid === props.pluginUuid && r.componentName === props.componentName,
  ),
);

const title = computed(
  () => registration.value?.label ?? loaded.value?.plugin.name ?? t('dashboard-widget-plugin-title'),
);
</script>

<template>
  <DashboardWidgetShell
    :title="title"
    :error="loaded ? null : t('dashboard-widget-plugin-unavailable')"
    :min-body-height="'6rem'"
  >
    <PluginSandboxFrame
      v-if="loaded"
      :plugin="loaded.plugin"
      :component="{ name: componentName, slot: 'dashboard.widget' }"
    />
  </DashboardWidgetShell>
</template>
