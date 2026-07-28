<script setup lang="ts">
/**
 * Plugin Slot Component
 *
 * Renders every plugin component registered for a slot `target`. Takes the
 * canonical dotted target name (a legacy alias is accepted and normalized) plus
 * a typed `context` bag, and provides the resolved slot name to descendants.
 */
import { computed, provide } from 'vue';
import { slotRegistrations } from '../loader';
import type { PluginSlotContext } from '../context';
import { canonicalSlotName } from '@nosdesk/core/types/plugin';
import type { PluginSlot, AnySlotName } from '@nosdesk/core/types/plugin';
import { pluginActivationKey } from '../usePluginActions';
import PluginSlotItem from './PluginSlotItem.vue';

const props = defineProps<{
  /** Canonical dotted slot name (e.g. `ticket.sidebar.panel`); an alias works too. */
  target: AnySlotName;
  /** Host-provided context; the mount fills the field its slot declares. */
  context?: PluginSlotContext;
  /** Per-component activation counters from `usePluginActions`. */
  actionActivatedMap?: ReadonlyMap<string, number>;
  /** Render only the given plugin's contributions (e.g. a plugin's own config
   *  page on its detail view). Omit to render every plugin's contribution. */
  pluginUuid?: string;
}>();

// Registrations are keyed by canonical name; normalize an alias-passed target.
const canonical = computed(() => canonicalSlotName(props.target) ?? props.target);
const registrations = computed(() => {
  const all = slotRegistrations.get(canonical.value as PluginSlot) ?? [];
  return props.pluginUuid ? all.filter((r) => r.pluginUuid === props.pluginUuid) : all;
});

provide('pluginSlot', canonical);
</script>

<template>
  <PluginSlotItem
    v-for="reg in registrations"
    :key="`${reg.pluginUuid}-${reg.componentName}`"
    :registration="reg"
    :slot-name="canonical"
    :context="context"
    :action-activated="actionActivatedMap?.get(pluginActivationKey(reg.pluginUuid, reg.componentName))"
  />
</template>
