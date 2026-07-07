<!--
Shared button primitive. One button system across the app so action
emphasis (primary / secondary / danger) and sizing stay consistent
instead of drifting across hand-rolled utility-class strings.

Variants:
  - primary       solid accent, the one main action per section
  - secondary     neutral filled, lower-emphasis affirmative actions
  - danger        solid red, prominent destructive CTAs (modal confirms)
  - warning       solid amber, reversible-but-consequential confirms
                  (reset to defaults, discard changes)
  - ghost         transparent neutral, low-emphasis / icon actions
  - ghost-danger  transparent red text, low-emphasis destructive actions
                  in dense rows (Revoke, Remove) where a filled red button
                  would be too heavy

`loading` shows an inline Spinner and disables the button; `icon` renders
a leading Icon. Use `aria-label` for icon-only buttons.
-->
<script setup lang="ts">
import { computed } from 'vue';
import Spinner from '@/components/common/Spinner.vue';
import Icon from '@/components/common/Icon.vue';
import type { IconName } from '@/components/common/icons';

type Variant = 'primary' | 'secondary' | 'danger' | 'warning' | 'ghost' | 'ghost-danger';
type Size = 'sm' | 'md' | 'lg';

interface Props {
  variant?: Variant;
  size?: Size;
  type?: 'button' | 'submit' | 'reset';
  disabled?: boolean;
  loading?: boolean;
  /** Stretch to the container width. */
  block?: boolean;
  /** Optional leading icon (replaced by the spinner while loading). */
  icon?: IconName;
  /** Required for icon-only buttons (no slot text). */
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

const sizeClasses: Record<Size, string> = {
  sm: 'text-xs px-3 py-1.5 gap-1.5',
  md: 'text-sm px-4 py-2 gap-2',
  lg: 'text-sm px-4 py-2.5 gap-2',
};

// `text-on-accent` not `text-white`. The on-accent foreground is
// a theme-aware token: the CSS injector picks black or white per
// theme via WCAG luminance against the actual accent fill (see
// `themes/utils/cssInjector.ts::pickAccentForeground`). Brand
// orange `#FF6B1A` resolves to black (7.4:1 AAA); themes whose
// accent is dark (epaper's `#000000`, gruvbox-dark) resolve to
// white. Workspace branding overrides recompute automatically.
//
// `bg-status-error` (`#EF4444`) on white is 3.76:1 — also under
// AA. White on `#EF4444` is 3.86:1 — also under AA. Neither
// passes; tightening to a darker red (`#B91C1C`) would give
// white 7.05:1 AAA, but that recolours the existing button. For
// now keep `text-white` on danger and revisit when we audit the
// status palette holistically.
const variantClasses: Record<Variant, string> = {
  primary: 'bg-accent text-on-accent hover:opacity-90',
  secondary: 'bg-surface-alt text-primary border border-default hover:bg-surface-hover',
  // `text-white` on danger/warning: same contrast tradeoff documented
  // above (no theme-aware on-status token yet). Kept as-is so the
  // existing filled-status buttons don't recolour.
  danger: 'bg-status-error text-white hover:opacity-90',
  warning: 'bg-status-warning text-white hover:opacity-90',
  ghost: 'text-secondary hover:text-primary hover:bg-surface-hover',
  'ghost-danger': 'text-status-error hover:bg-status-error/10',
};

// Match the icon/spinner weight to the button size.
const iconSize = computed(() => (props.size === 'sm' ? 'xs' : 'sm'));
</script>

<template>
  <button
    :type="type"
    :disabled="disabled || loading"
    :aria-label="ariaLabel"
    :class="[
      'inline-flex items-center justify-center whitespace-nowrap font-medium rounded-lg transition-colors',
      'focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface',
      'disabled:opacity-50 disabled:cursor-not-allowed',
      sizeClasses[size],
      variantClasses[variant],
      block ? 'w-full' : '',
    ]"
  >
    <Spinner v-if="loading" :size="iconSize" />
    <Icon v-else-if="icon" :name="icon" :size="iconSize" />
    <span v-if="$slots.default"><slot /></span>
  </button>
</template>
