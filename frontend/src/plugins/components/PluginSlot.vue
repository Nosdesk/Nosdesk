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
import type { PluginSlot } from '@nosdesk/core/types/plugin';
import { pluginActivationKey } from '../usePluginActions';
import PluginSlotItem from './PluginSlotItem.vue';

const props = defineProps<{
  /** Canonical dotted slot name (e.g. `ticket.sidebar.panel`); an alias works too. */
  target: string;
  /** Host-provided context; the mount fills the field its slot declares. */
  context?: PluginSlotContext;
  /** Per-component activation counters from `usePluginActions`. */
  actionActivatedMap?: ReadonlyMap<string, number>;
}>();

// Registrations are keyed by canonical name; normalize an alias-passed target.
const canonical = computed(() => canonicalSlotName(props.target) ?? props.target);
const registrations = computed(() => slotRegistrations.get(canonical.value as PluginSlot) ?? []);

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
