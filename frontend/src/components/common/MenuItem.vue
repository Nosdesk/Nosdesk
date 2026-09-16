<!--
One row of a popup menu, for menus whose rows carry their own content (a
checkbox, a two-line label, a trailing chevron) and so cannot be expressed
as a MenuList item. Wears the shared menu recipe, so it lines up with
MenuList rows in density, tap target and hover chrome.

Native handlers and ARIA attributes fall through to the root <button>;
`checked` sets `aria-checked` for the checkbox and radio roles.
-->
<script setup lang="ts">
import { computed } from 'vue';
import { menuItem, type MenuItemTone } from '@/recipes/menu';

interface Props {
  role?: 'menuitem' | 'menuitemcheckbox' | 'menuitemradio';
  tone?: MenuItemTone;
  /** Keyboard highlight from a roving index (not DOM focus). */
  highlighted?: boolean;
  /** Two-line rows: keep the leading control on the first line. */
  align?: 'center' | 'start';
  disabled?: boolean;
  /** For the checkbox and radio roles. */
  checked?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  role: 'menuitem',
  tone: 'default',
  highlighted: false,
  align: 'center',
  disabled: false,
  checked: undefined,
});

const classes = computed(() =>
  menuItem({ tone: props.tone, highlighted: props.highlighted, align: props.align }),
);
const ariaChecked = computed(() => (props.role === 'menuitem' ? undefined : props.checked));
</script>

<template>
  <button
    type="button"
    :role="role"
    :aria-checked="ariaChecked"
    :disabled="disabled"
    :data-tone="tone"
    :data-highlighted="highlighted || undefined"
    :class="classes"
  >
    <slot />
  </button>
</template>
