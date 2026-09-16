<!--
Navigation styled as a button: a RouterLink (`to`) or an anchor (`href`)
wearing the shared button recipe. It stays a link semantically, which is
what a screen reader and a middle-click expect of navigation; that is why
this is a sibling of Button rather than an `as` prop on it.
-->
<script setup lang="ts">
import { computed } from 'vue';
import { RouterLink, type RouteLocationRaw } from 'vue-router';
import Icon from '@/components/common/Icon.vue';
import type { IconName } from '@/components/common/icons';
import { button, buttonIconSize, type ButtonSize, type ButtonVariant } from '@/recipes/button';

interface Props {
  /** In-app destination. Takes precedence over `href`. */
  to?: RouteLocationRaw;
  /** External or non-router destination. */
  href?: string;
  variant?: ButtonVariant;
  size?: ButtonSize;
  block?: boolean;
  icon?: IconName;
}

const props = withDefaults(defineProps<Props>(), {
  variant: 'primary',
  size: 'md',
  block: false,
});

if (import.meta.env.DEV && !props.to && !props.href) {
  console.warn('[LinkButton] needs `to` or `href`; without one it is not a link');
}

/** External anchors open in a new tab without handing the opener over. */
const external = computed(() => !!props.href && /^[a-z]+:/i.test(props.href));

const classes = computed(() =>
  button({ variant: props.variant, size: props.size, block: props.block }),
);
const iconSize = computed(() => buttonIconSize(props.size));
</script>

<template>
  <RouterLink
    v-if="to"
    :to="to"
    :data-variant="variant"
    :data-size="size"
    :class="classes"
  >
    <Icon v-if="icon" :name="icon" :size="iconSize" />
    <span><slot /></span>
  </RouterLink>
  <a
    v-else
    :href="href"
    :target="external ? '_blank' : undefined"
    :rel="external ? 'noopener noreferrer' : undefined"
    :data-variant="variant"
    :data-size="size"
    :class="classes"
  >
    <Icon v-if="icon" :name="icon" :size="iconSize" />
    <span><slot /></span>
  </a>
</template>
