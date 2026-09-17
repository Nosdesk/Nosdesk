<!--
Tab strip on Reka's Tabs, styled by `recipes/tabs.ts`. Reka provides the
tablist/tab roles, roving focus with the arrow keys, Home and End, and
automatic activation on focus (manual with `activationMode="manual"`).
The active tab is `v-model`; route-driven tab bars bind it to the route
and navigate in the update handler, so there is no TabsContent here and
no `aria-controls` to satisfy.

Items render as icon + label + optional badge; the `item` slot replaces
that for bespoke content and receives `{ item, active }`.
-->
<script setup lang="ts" generic="T extends string = string">
import { computed } from 'vue';
import { TabsIndicator, TabsList, TabsRoot, TabsTrigger } from 'reka-ui';
import Icon from '@/components/common/Icon.vue';
import type { IconName } from '@/components/common/icons';
import { tabsIndicator, tabsList, tabsTrigger, type TabsSize, type TabsVariant } from '@/recipes/tabs';

export interface TabBarItem<T extends string = string> {
  value: T;
  label: string;
  icon?: IconName;
  /** Count or short text shown after the label. */
  badge?: string | number;
  disabled?: boolean;
}

const props = withDefaults(
  defineProps<{
    modelValue: T;
    items: readonly TabBarItem<T>[];
    variant?: TabsVariant;
    size?: TabsSize;
    /** Accessible name for the tablist. */
    label: string;
    activationMode?: 'automatic' | 'manual';
    /** Extra classes for the list (layout hooks like `-mx-2` or `hidden lg:flex`). */
    listClass?: string;
  }>(),
  {
    variant: 'underline',
    size: 'md',
    activationMode: 'automatic',
  },
);

const emit = defineEmits<{ (e: 'update:modelValue', value: T): void }>();

const listClasses = computed(() => tabsList({ variant: props.variant }));
const triggerClasses = computed(() => tabsTrigger({ variant: props.variant, size: props.size }));

function onUpdate(value: string | number) {
  emit('update:modelValue', value as T);
}
</script>

<template>
  <TabsRoot :model-value="modelValue" :activation-mode="activationMode" @update:model-value="onUpdate">
    <TabsList :aria-label="label" :class="[listClasses, listClass]">
      <TabsIndicator v-if="variant === 'underline'" :class="tabsIndicator" aria-hidden="true" />
      <TabsTrigger
        v-for="item in items"
        :key="item.value"
        :value="item.value"
        :disabled="item.disabled"
        :class="triggerClasses"
      >
        <slot name="item" :item="item" :active="item.value === modelValue">
          <Icon v-if="item.icon" :name="item.icon" class="w-3.5 h-3.5 shrink-0" aria-hidden="true" />
          <span>{{ item.label }}</span>
          <span
            v-if="item.badge !== undefined && item.badge !== ''"
            class="rounded-full bg-accent/15 px-1.5 py-0.5 text-2xs font-semibold leading-none text-accent"
          >
            {{ item.badge }}
          </span>
        </slot>
      </TabsTrigger>
    </TabsList>
  </TabsRoot>
</template>
