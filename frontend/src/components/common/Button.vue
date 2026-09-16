<!--
Shared button primitive. Visual identity comes from `recipes/button.ts`
(one recipe shared with LinkButton and IconButton); this component adds
the native button behaviour: `type`, `disabled`, `loading` (inline
Spinner, disabled while pending) and an optional leading `icon`.

For an icon with no label use IconButton, which makes the accessible
name a required prop. For navigation styled as a button use LinkButton.
-->
<script setup lang="ts">
import { computed } from 'vue';
import Spinner from '@/components/common/Spinner.vue';
import Icon from '@/components/common/Icon.vue';
import type { IconName } from '@/components/common/icons';
import { button, buttonIconSize, type ButtonSize, type ButtonVariant } from '@/recipes/button';

interface Props {
  variant?: ButtonVariant;
  size?: ButtonSize;
  type?: 'button' | 'submit' | 'reset';
  disabled?: boolean;
  loading?: boolean;
  /** Stretch to the container width. */
  block?: boolean;
  /** Optional leading icon (replaced by the spinner while loading). */
  icon?: IconName;
  /** Accessible name when the slot carries no text. Prefer IconButton. */
  ariaLabel?: string;
}

const props = withDefaults(defineProps<Props>(), {
  variant: 'primary',
  size: 'md',
  type: 'button',
  disabled: false,
  loading: false,
  block: false,
});

// No click emit: with a single root <button>, a parent's @click (and any
// other native handler/attribute) falls through to it automatically.
// Re-emitting would be redundant and risks double-firing.

const classes = computed(() =>
  button({ variant: props.variant, size: props.size, block: props.block }),
);
const iconSize = computed(() => buttonIconSize(props.size));
</script>

<template>
  <button
    :type="type"
    :disabled="disabled || loading"
    :aria-label="ariaLabel"
    :data-variant="variant"
    :data-size="size"
    :class="classes"
  >
    <Spinner v-if="loading" :size="iconSize" />
    <Icon v-else-if="icon" :name="icon" :size="iconSize" />
    <span v-if="$slots.default"><slot /></span>
  </button>
</template>
