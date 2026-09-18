<!--
One row of a popup menu. Wears the shared menu recipe, so every row in the
app lines up in density, tap target and hover chrome.

Inside a Reka DropdownMenu (the desktop `ResponsiveMenu` path) the row
becomes a `DropdownMenuItem` / `CheckboxItem`: roving focus, typeahead,
Enter/Space select and `data-highlighted` come from Reka. A radio row is
a CheckboxItem wearing `menuitemradio` (Reka's RadioItem needs a
RadioGroup above it, and the menus here hold their own selection). Anywhere else (the mobile sheet, a dialog panel) it is a plain
button with the ARIA role given, which is why the recipe carries the
highlight styling for both `data-highlighted` and the `highlighted` prop.

Native handlers fall through to the button either way; a Reka select is
prevented from closing the menu so consumers keep deciding when to close,
exactly as before.
-->
<script setup lang="ts">
import { computed } from 'vue';
import { DropdownMenuCheckboxItem, DropdownMenuItem, injectDropdownMenuRootContext } from 'reka-ui';
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

// Null outside a DropdownMenuRoot (the sheet, dialogs); Reka's inject
// with a fallback does not throw.
const inMenu = injectDropdownMenuRootContext(null) !== null;

const rekaPart = computed(() => {
  if (!inMenu) return null;
  if (props.role === 'menuitem') return DropdownMenuItem;
  return DropdownMenuCheckboxItem;
});

const rekaProps = computed(() => (props.role === 'menuitem' ? {} : { modelValue: props.checked ?? false }));

const classes = computed(() =>
  menuItem({ tone: props.tone, highlighted: props.highlighted, align: props.align }),
);
const ariaChecked = computed(() => (props.role === 'menuitem' ? undefined : props.checked));

// Consumers close the menu themselves after handling the click.
function keepOpen(event: Event) {
  event.preventDefault();
}
</script>

<template>
  <component
    :is="rekaPart"
    v-if="rekaPart"
    as-child
    :disabled="disabled"
    v-bind="rekaProps"
    @select="keepOpen"
  >
    <!-- The child's role wins over Reka's, so the radio row reads as one. -->
    <button
      type="button"
      :role="role"
      :disabled="disabled"
      :data-tone="tone"
      :class="classes"
    >
      <slot />
    </button>
  </component>
  <button
    v-else
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
