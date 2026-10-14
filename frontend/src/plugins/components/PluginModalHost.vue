<!--
PluginModalHost — the single host for the on-demand plugin modal surface.

Mounted once at app root. Watches the shared `pluginModal` request (opened by
action handlers via `openPluginModal`) and renders the requested plugin
component in a sandbox frame inside a Modal. One plugin modal at a time; the
user closes it (X / backdrop / Esc). If the plugin is no longer loaded, the
modal simply doesn't open.
-->
<script setup lang="ts">
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';
import Modal from '@/components/Modal.vue';
import PluginSandboxFrame from './PluginSandboxFrame.vue';
import { pluginModal, closePluginModal } from '../usePluginModal';
import { getLoadedPlugin, isComponentVisible } from '../loader';

const fluent = useFluent();
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args);

// Only open for a plugin that's actually loaded (enabled), and for a component
// the viewer may see: one whose `audience` they don't meet never opens, however
// the request was made.
const loaded = computed(() => {
  const req = pluginModal.value;
  if (!req || !isComponentVisible(req.pluginUuid, req.componentName, req.slot)) return undefined;
  return getLoadedPlugin(req.pluginUuid);
});
const open = computed(() => !!pluginModal.value && !!loaded.value);

// A computed, not an inline literal in the template: the literal sits in the
// Modal's slot, which re-renders whenever another dialog stacks over it, and a
// new object each time would read to the frame as a new component.
const component = computed(() =>
  pluginModal.value
    ? { name: pluginModal.value.componentName, slot: pluginModal.value.slot }
    : null,
);

const title = computed(
  () => pluginModal.value?.title ?? loaded.value?.plugin.name ?? t('plugin-modal-title'),
);
</script>

<template>
  <Modal :show="open" :title="title" size="lg" @close="closePluginModal">
    <PluginSandboxFrame
      v-if="open && pluginModal && loaded && component"
      :key="`${pluginModal.pluginUuid}:${pluginModal.componentName}`"
      :plugin="loaded.plugin"
      :component="component"
      :context="pluginModal.context"
    />
  </Modal>
</template>
