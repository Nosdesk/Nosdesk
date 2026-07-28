<!--
PluginPageView — the full-page host for a plugin `nav.item` contribution.

Routed at /plugins/:uuid/pages/:component. Resolves the loaded plugin + the
named component from the route params and renders it full-page in a sandbox
frame. If the plugin isn't loaded (disabled / uninstalled) or doesn't declare a
nav.item component by that name, shows a not-available state rather than a blank
page.
-->
<script setup lang="ts">
import { computed } from 'vue';
import { useRoute } from 'vue-router';
import { useFluent } from 'fluent-vue';
import PluginSandboxFrame from '@/plugins/components/PluginSandboxFrame.vue';
import NotFoundIllustration from '@/components/common/NotFoundIllustration.vue';
import { getLoadedPlugin, getSlotRegistrations } from '@/plugins/loader';

const route = useRoute();
const fluent = useFluent();
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args);

const uuid = computed(() => String(route.params.uuid ?? ''));
const componentName = computed(() => String(route.params.component ?? ''));

const loaded = computed(() => (uuid.value ? getLoadedPlugin(uuid.value) : undefined));

// The component must actually be a declared nav.item on this plugin, so a route
// can't render an arbitrary component full-page.
const registration = computed(() =>
  getSlotRegistrations('nav.item').find(
    (r) => r.pluginUuid === uuid.value && r.componentName === componentName.value,
  ),
);

const available = computed(() => !!loaded.value && !!registration.value);
const title = computed(() => registration.value?.label ?? loaded.value?.plugin.name ?? '');
</script>

<template>
  <div class="h-full">
    <div v-if="available && loaded" class="mx-auto max-w-5xl p-4">
      <h1 v-if="title" class="mb-4 text-lg font-semibold text-primary">{{ title }}</h1>
      <PluginSandboxFrame
        :key="`${uuid}:${componentName}`"
        :plugin="loaded.plugin"
        :component="{ name: componentName, slot: 'nav.item' }"
      />
    </div>
    <div v-else class="flex h-full flex-col items-center justify-center gap-3 p-8 text-center">
      <NotFoundIllustration class="w-40" />
      <p class="text-sm text-secondary">{{ t('plugin-page-unavailable') }}</p>
    </div>
  </div>
</template>
