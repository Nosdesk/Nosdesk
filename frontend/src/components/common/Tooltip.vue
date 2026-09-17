<!--
Hover/focus hint on any trigger, built on Reka's Tooltip: reachable by
keyboard focus (touch has no hover; the trigger keeps its accessible name), `aria-describedby` wired to the trigger, opens on the
shared provider delay (App.vue) and at once when moving between triggers.

The default slot is the trigger and must be a single element or
component; Reka merges the trigger behaviour into it (`asChild`). The
tooltip is supplementary, so the trigger still needs its own accessible
name; `IconButton` passes its `label` for both.

Native `title` is not this: it is unreachable by keyboard and touch and
cannot be styled. Use `text` here instead.
-->
<script setup lang="ts">
import { computed } from 'vue';
import { TooltipContent, TooltipPortal, TooltipRoot, TooltipTrigger } from 'reka-ui';
import { tooltip, type TooltipRecipe } from '@/recipes/tooltip';

interface Props {
  text: string;
  side?: 'top' | 'right' | 'bottom' | 'left';
  align?: 'start' | 'center' | 'end';
  tone?: TooltipRecipe['tone'];
  /** Render the trigger alone (keeps call sites stable when a hint is
   *  conditional, e.g. a visible label already sits beside the control). */
  disabled?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  side: 'top',
  align: 'center',
  tone: 'default',
  disabled: false,
});

const classes = computed(() => tooltip({ tone: props.tone }));
</script>

<template>
  <slot v-if="disabled || !text" />
  <TooltipRoot v-else>
    <TooltipTrigger as-child>
      <slot />
    </TooltipTrigger>
    <TooltipPortal>
      <TooltipContent :side="side" :align="align" :side-offset="6" :class="classes">
        {{ text }}
      </TooltipContent>
    </TooltipPortal>
  </TooltipRoot>
</template>
