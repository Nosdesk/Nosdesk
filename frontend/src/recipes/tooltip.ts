/**
 * The tooltip surface recipe. One look for every hover/focus hint, on
 * Reka's `TooltipContent`; enter/exit motion comes from the `data-state`
 * keyframes in `assets/main.css` (`reka-fade-in` / `reka-fade-out`), so
 * the recipe carries only paint.
 *
 * Tooltips are supplementary: the trigger keeps its own accessible name,
 * the tooltip repeats or extends it for sighted users.
 */
import { cva, type VariantProps } from 'class-variance-authority';

export const tooltip = cva(
  [
    'z-overlay max-w-xs rounded-md border border-default bg-surface px-2 py-1 text-xs text-primary shadow-lg select-none',
    'data-[state=delayed-open]:animate-reka-fade-in data-[state=instant-open]:animate-reka-fade-in data-[state=closed]:animate-reka-fade-out',
  ],
  {
    variants: {
      tone: {
        default: '',
        // Longer explanatory hints; wraps rather than truncates.
        detail: 'max-w-sm whitespace-normal leading-snug',
      },
    },
    defaultVariants: { tone: 'default' },
  },
);

export type TooltipRecipe = VariantProps<typeof tooltip>;
