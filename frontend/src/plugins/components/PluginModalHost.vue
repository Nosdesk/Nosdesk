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
import { getLoadedPlugin } from '../loader';

const fluent = useFluent();
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args);

// Only open for a plugin that's actually loaded (enabled).
const loaded = computed(() => {
  const req = pluginModal.value;
  return req ? getLoadedPlugin(req.pluginUuid) : undefined;
});
const open = computed(() => !!pluginModal.value && !!loaded.value);

const title = computed(
  () => pluginModal.value?.title ?? loaded.value?.plugin.name ?? t('plugin-modal-title'),
);
</script>

<template>
  <Modal :show="open" :title="title" size="lg" @close="closePluginModal">
    <PluginSandboxFrame
      v-if="open && pluginModal && loaded"
      :key="`${pluginModal.pluginUuid}:${pluginModal.componentName}`"
      :plugin="loaded.plugin"
      :component="{ name: pluginModal.componentName, slot: pluginModal.slot }"
      :context="pluginModal.context"
    />
  </Modal>
</template>
