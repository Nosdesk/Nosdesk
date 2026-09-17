<!--
Icon-only button. The accessible name is a required prop, so an unnamed
icon button is a compile error rather than a lint finding; it is also the
tooltip (a real one, reachable by keyboard focus, not `title`). Square
padding and a touch-only tap-target floor come from the shared recipe
(`iconOnly`), so an IconButton lines up with a Button of the same size.

Defaults to `ghost`, the low-emphasis chrome most icon actions want.

Attributes and listeners fall through to the `<button>`, not the tooltip
wrapper, so `@click` and `class` behave as on a plain button.
-->
<script setup lang="ts">
import { computed } from 'vue';
import Spinner from '@/components/common/Spinner.vue';
import Icon from '@/components/common/Icon.vue';
import Tooltip from '@/components/common/Tooltip.vue';
import type { IconName } from '@/components/common/icons';
import { button, buttonIconSize, type ButtonSize, type ButtonVariant } from '@/recipes/button';

defineOptions({ inheritAttrs: false });

interface Props {
  /** Accessible name; also the tooltip text. */
  label: string;
  icon: IconName;
  variant?: ButtonVariant;
  size?: ButtonSize;
  type?: 'button' | 'submit' | 'reset';
  disabled?: boolean;
  loading?: boolean;
  /** Suppress the tooltip where the label is already visible beside the
   *  control or a hint would get in the way (dense rows, mobile bars). */
  tooltip?: boolean;
  tooltipSide?: 'top' | 'right' | 'bottom' | 'left';
}

const props = withDefaults(defineProps<Props>(), {
  variant: 'ghost',
  size: 'md',
  type: 'button',
  disabled: false,
  loading: false,
  tooltip: true,
  tooltipSide: 'top',
});

const classes = computed(() =>
  button({ variant: props.variant, size: props.size, iconOnly: true }),
);
const iconSize = computed(() => buttonIconSize(props.size));
</script>

<template>
  <Tooltip :text="label" :side="tooltipSide" :disabled="!tooltip">
    <button
      v-bind="$attrs"
      :type="type"
      :disabled="disabled || loading"
      :aria-label="label"
      :data-variant="variant"
      :data-size="size"
      data-icon-only
      :class="classes"
    >
      <Spinner v-if="loading" :size="iconSize" />
      <Icon v-else :name="icon" :size="iconSize" />
    </button>
  </Tooltip>
</template>
