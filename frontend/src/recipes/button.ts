/**
 * The button recipe: the one place a button's visual identity is defined.
 *
 * `Button`, `LinkButton` and `IconButton` all render `button(...)`, and each
 * stamps `data-variant` / `data-size` on its root so themes and tests hook
 * the variant (`button[data-variant="danger"]`), never the utility classes
 * that happen to implement it today.
 *
 * Consumers may add layout classes (`ml-auto`, `w-full`, `shrink-0`) but
 * never colour or padding; that drift is what the recipe exists to remove,
 * and `scripts/check-buttons.mjs` flags raw buttons that re-implement a
 * variant.
 *
 * Variants:
 *   primary       solid accent, the one main action per section
 *   secondary     neutral filled, lower-emphasis affirmative actions
 *   danger        solid red, prominent destructive CTAs (modal confirms)
 *   warning       solid amber, reversible-but-consequential confirms
 *   ghost         transparent neutral, low-emphasis and icon actions
 *   ghost-danger  transparent red text, destructive actions in dense rows
 *
 * Colour notes: `text-on-accent` is the theme-aware foreground for the
 * accent fill (see `themes/utils/cssInjector.ts::pickAccentForeground`).
 * `text-white` on danger/warning is a known AA shortfall on `#EF4444`;
 * fixing it recolours the status palette and is tracked separately.
 *
 * Focus and disabled: the base layer in `assets/main.css` gives every
 * button a focus outline and the disabled cursor/opacity. The recipe
 * restates only the focus ring, because an offset ring reads better than
 * the outline on a filled button.
 *
 * Tap targets: `pointer-coarse:min-h-*` is a floor on touch input only.
 * Apple and Google both put the minimum comfortable target near 9mm; a
 * padding-derived 37px button is 6.2mm on a tablet, so the padding alone
 * is not enough, while a mouse needs no floor.
 */
import { cva, type VariantProps } from 'class-variance-authority';

export const button = cva(
  [
    'inline-flex items-center justify-center whitespace-nowrap font-medium rounded-lg transition-colors',
    'focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface',
  ],
  {
    variants: {
      variant: {
        primary: 'bg-accent text-on-accent hover:opacity-90',
        secondary: 'bg-surface-alt text-primary border border-default hover:bg-surface-hover',
        danger: 'bg-status-error text-white hover:opacity-90',
        warning: 'bg-status-warning text-white hover:opacity-90',
        ghost: 'text-secondary hover:text-primary hover:bg-surface-hover',
        'ghost-danger': 'text-status-error hover:bg-status-error/10',
      },
      size: {
        sm: 'text-xs gap-1.5 pointer-coarse:min-h-9',
        md: 'text-sm gap-2 pointer-coarse:min-h-11',
        lg: 'text-sm gap-2 pointer-coarse:min-h-11',
      },
      /** Square padding for a button whose only content is an icon. */
      iconOnly: {
        true: '',
        false: '',
      },
      /** Stretch to the container width. */
      block: {
        true: 'w-full',
        false: '',
      },
    },
    compoundVariants: [
      { iconOnly: false, size: 'sm', class: 'px-3 py-1.5' },
      { iconOnly: false, size: 'md', class: 'px-4 py-2' },
      { iconOnly: false, size: 'lg', class: 'px-4 py-2.5' },
      { iconOnly: true, size: 'sm', class: 'p-1.5 pointer-coarse:min-w-9' },
      { iconOnly: true, size: 'md', class: 'p-2 pointer-coarse:min-w-11' },
      { iconOnly: true, size: 'lg', class: 'p-2.5 pointer-coarse:min-w-11' },
    ],
    defaultVariants: {
      variant: 'primary',
      size: 'md',
      iconOnly: false,
      block: false,
    },
  },
);

export type ButtonRecipe = VariantProps<typeof button>;
export type ButtonVariant = NonNullable<ButtonRecipe['variant']>;
export type ButtonSize = NonNullable<ButtonRecipe['size']>;

/** Icon and spinner weight that matches each button size. */
export function buttonIconSize(size: ButtonSize): 'xs' | 'sm' {
  return size === 'sm' ? 'xs' : 'sm';
}
