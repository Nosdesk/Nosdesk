<!--
Icon-only button. The accessible name is a required prop, so an unnamed
icon button is a compile error rather than a lint finding; it doubles as
the tooltip. Square padding and a touch-only tap-target floor come from
the shared recipe (`iconOnly`), so an IconButton lines up with a Button
of the same size.

Defaults to `ghost`, the low-emphasis chrome most icon actions want.
-->
<script setup lang="ts">
import { computed } from 'vue';
import Spinner from '@/components/common/Spinner.vue';
import Icon from '@/components/common/Icon.vue';
import type { IconName } from '@/components/common/icons';
import { button, buttonIconSize, type ButtonSize, type ButtonVariant } from '@/recipes/button';

interface Props {
  /** Accessible name; also the tooltip. */
  label: string;
  icon: IconName;
  variant?: ButtonVariant;
  size?: ButtonSize;
  type?: 'button' | 'submit' | 'reset';
  disabled?: boolean;
  loading?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  variant: 'ghost',
  size: 'md',
  type: 'button',
  disabled: false,
  loading: false,
});

const classes = computed(() =>
  button({ variant: props.variant, size: props.size, iconOnly: true }),
);
const iconSize = computed(() => buttonIconSize(props.size));
</script>

<template>
  <button
    :type="type"
    :disabled="disabled || loading"
    :aria-label="label"
    :title="label"
    :data-variant="variant"
    :data-size="size"
    data-icon-only
    :class="classes"
  >
    <Spinner v-if="loading" :size="iconSize" />
    <Icon v-else :name="icon" :size="iconSize" />
  </button>
</template>
