/**
 * The tab-bar recipe: the one place tab lists and tab triggers are
 * styled, on Reka's Tabs. Two looks exist in the app:
 *
 * - `underline`: a text row with the active tab underlined. The line is
 *   the `TabsIndicator` sliding on Reka's position variables, so the
 *   trigger itself carries only text colour.
 * - `pill`: a small rounded group where the active tab sits on a raised
 *   surface (the tickets and asset headers). No indicator; the trigger
 *   paints its own active state from `data-state`.
 *
 * Density: `sm` is the compact header size, `md` the page size; touch
 * gets a 44px floor on the page size.
 */
import { cva, type VariantProps } from 'class-variance-authority';

export const tabsList = cva('flex items-center', {
  variants: {
    variant: {
      underline: 'relative gap-1 border-b border-default',
      pill: 'gap-0.5 rounded-md bg-surface-alt p-0.5',
    },
  },
  defaultVariants: { variant: 'underline' },
});

export const tabsTrigger = cva(
  [
    'relative inline-flex items-center gap-1.5 font-medium whitespace-nowrap shrink-0 transition-colors outline-none',
    'focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset',
    'data-[disabled]:opacity-50 data-[disabled]:cursor-not-allowed',
  ],
  {
    variants: {
      variant: {
        underline: 'text-tertiary hover:text-secondary data-[state=active]:text-primary',
        pill: [
          'rounded text-secondary hover:text-primary hover:bg-surface/60',
          'data-[state=active]:bg-surface data-[state=active]:text-primary data-[state=active]:shadow-sm',
        ],
      },
      size: {
        sm: 'text-xs',
        md: 'text-sm',
      },
    },
    compoundVariants: [
      { variant: 'underline', size: 'sm', class: 'px-3 py-2 sm:py-1.5' },
      { variant: 'underline', size: 'md', class: 'px-3 py-2.5 min-h-[44px] sm:min-h-0 sm:py-2' },
      { variant: 'pill', size: 'sm', class: 'px-2.5 h-7' },
      { variant: 'pill', size: 'md', class: 'px-3 h-8' },
    ],
    defaultVariants: { variant: 'underline', size: 'md' },
  },
);

/** The sliding underline. Position and size come from Reka's variables. */
export const tabsIndicator =
  'absolute bottom-0 left-0 h-0.5 rounded-t bg-accent transition-[width,transform] duration-200 ease-out ' +
  'w-[var(--reka-tabs-indicator-size)] translate-x-[var(--reka-tabs-indicator-position)] motion-reduce:transition-none';

export type TabsRecipe = VariantProps<typeof tabsTrigger>;
export type TabsVariant = NonNullable<TabsRecipe['variant']>;
export type TabsSize = NonNullable<TabsRecipe['size']>;
