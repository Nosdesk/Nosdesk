<script setup lang="ts">
/**
 * Plugin Slot Item
 *
 * Renders one registered plugin component in a slot. Every plugin — every trust
 * tier — runs in the opaque-origin sandbox via PluginSandboxFrame; the in-process
 * Vue-component path was removed in the sandbox-all migration (4b-3c-4).
 *
 * This component owns the HOST CHROME around that frame. When the resolved
 * chrome is `card` the frame is wrapped in the app's `SectionCard`, the same
 * primitive every built-in card uses, so a plugin panel is spaced, bordered and
 * titled identically to its neighbours without the plugin drawing any of it.
 * When it is `none` the bare frame is mounted, for contributions whose host
 * already supplies chrome or that sit inline inside another card.
 */
import { computed, onErrorCaptured, ref } from 'vue';
import { getLoadedPlugin, type PluginSlotRegistration } from '../loader';
import type { PluginSlotContext } from '../context';
import PluginSandboxFrame from './PluginSandboxFrame.vue';
import PluginIcon from '@/components/plugins/PluginIcon.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import { logger } from '@nosdesk/core/utils/logger';

const props = defineProps<{
  registration: PluginSlotRegistration;
  slotName: string;
  context?: PluginSlotContext;
  actionActivated?: number;
}>();

const error = ref<string | null>(null);
onErrorCaptured((err) => {
  logger.error('Plugin error:', err);
  error.value = err instanceof Error ? err.message : String(err);
  return false;
});

const loaded = getLoadedPlugin(props.registration.pluginUuid);

// Latest height the plugin reported: null until the first report, 0 when it
// rendered nothing.
const contentHeight = ref<number | null>(null);

/** A plugin that rendered nothing should leave no trace, not an empty card.
 *  Only true once the plugin has actually reported 0 — before the first report
 *  the chrome renders, so a panel with content never flashes in late. */
const isEmpty = computed(() => contentHeight.value === 0 && !error.value);

const withCard = computed(() => props.registration.chrome === 'card');

const title = computed(
  () => props.registration.label || loaded?.plugin.display_name || props.registration.pluginName,
);
</script>

<template>
  <!-- Collapsed to zero height when the plugin draws nothing, with the frame
       still MOUNTED inside: a plugin that starts empty and fills in after a
       fetch has to keep reporting, and unmounting the iframe would strand it
       empty forever (it would re-mount, draw nothing, and report empty again).

       Zero height rather than `display: none` on purpose. Hiding suspends
       layout inside the iframe, so the guest could no longer measure itself and
       needed a third protocol state to ask for layout back plus a timer loop to
       chase the height once it returned. Clipping keeps layout alive, so a
       late-filling plugin is just another content change. The cost is that
       these stacks are `flex flex-col gap-3`, so a collapsed item still leaves
       one gap's worth of space; that is invisible whitespace between two cards,
       and far cheaper than the machinery it replaces. -->
  <div
    class="plugin-slot-item"
    :class="{ 'plugin-slot-item--empty': isEmpty }"
    :aria-hidden="isEmpty || undefined"
    :data-plugin="registration.pluginName"
    :data-component="registration.componentName"
  >
    <!-- Body padding is left at SectionCard's default `p-3`, so a plugin panel
         insets exactly like every built-in card and the plugin adds none of its
         own. Content that genuinely needs the full bleed declares
         `chrome: "none"` and draws its own frame. -->
    <SectionCard v-if="withCard">
      <template #leading>
        <PluginIcon :uuid="registration.pluginUuid" :alt="title" size="xs" />
      </template>
      <template #title>{{ title }}</template>

      <div v-if="error" class="px-4 py-6 text-center text-xs text-status-error">
        {{ error }}
      </div>
      <PluginSandboxFrame
        v-else-if="loaded"
        :plugin="loaded.plugin"
        :component="{ name: registration.componentName, slot: slotName }"
        :context="context"
        :actionActivated="actionActivated"
        @content-height="contentHeight = $event"
      />
    </SectionCard>

    <template v-else>
      <div v-if="error" class="px-4 py-6 text-center text-xs text-status-error">
        {{ error }}
      </div>
      <PluginSandboxFrame
        v-else-if="loaded"
        :plugin="loaded.plugin"
        :component="{ name: registration.componentName, slot: slotName }"
        :context="context"
        :actionActivated="actionActivated"
        @content-height="contentHeight = $event"
      />
    </template>
  </div>
</template>

<style scoped>
.plugin-slot-item {
  contain: layout style;
}

/* Collapsed, not unmounted and not hidden — see the template comment. Clipping
   keeps the frame laid out, which is what lets the guest keep measuring. */
.plugin-slot-item--empty {
  block-size: 0;
  overflow: hidden;
  pointer-events: none;
}
</style>
