/**
 * The menu-row recipe: the one place a popup menu's rows are styled.
 *
 * `MenuList` (data-driven rows) and `MenuItem` (slot-driven rows for menus
 * whose content does not fit a flat item list) both render `menuItem(...)`,
 * so every popup menu in the app has the same density, tap-target floor
 * and hover chrome. Rows are comfortable on touch (44px floor, `py-2.5`,
 * `text-sm`) and dense from `md:` up, matching MenuList's long-standing
 * chrome.
 *
 * `tone` is the row's meaning (destructive, toggled on), `highlighted` is
 * keyboard focus tracked by a roving index rather than DOM focus, and the
 * two never fight: a highlighted row shows the accent wash and drops the
 * hover wash, which is what the roving-index menus already did by hand.
 *
 * Disabled cursor and opacity come from the base layer in `assets/main.css`.
 */
import { cva, type VariantProps } from 'class-variance-authority';

export const menuItem = cva(
  'w-full px-3 py-2.5 md:py-1.5 min-h-[44px] md:min-h-0 flex gap-2 text-left text-sm md:text-xs transition-colors',
  {
    variants: {
      tone: {
        default: 'text-secondary hover:text-primary',
        danger: 'text-status-error',
        active: 'text-accent hover:text-accent-hover',
      },
      highlighted: {
        true: 'bg-accent/10',
        false: '',
      },
      /** `start` for two-line rows whose leading control should sit on the first line. */
      align: {
        center: 'items-center',
        start: 'items-start',
      },
    },
    compoundVariants: [
      { tone: 'default', highlighted: false, class: 'hover:bg-surface-hover' },
      { tone: 'active', highlighted: false, class: 'hover:bg-surface-hover' },
      { tone: 'danger', highlighted: false, class: 'hover:bg-status-error/10' },
    ],
    defaultVariants: {
      tone: 'default',
      highlighted: false,
      align: 'center',
    },
  },
);

export type MenuItemRecipe = VariantProps<typeof menuItem>;
export type MenuItemTone = NonNullable<MenuItemRecipe['tone']>;
