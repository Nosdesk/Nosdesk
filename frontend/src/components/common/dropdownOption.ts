/**
 * Option shape for BaseDropdown (and SearchableDropdown). Values are
 * strings by default; number-valued lists pass `T = number`. A "no
 * selection" choice is an explicit sentinel option, not `null`: the
 * underlying Select treats a null model as nothing selected.
 */
export interface DropdownOption<T extends string | number = string> {
  value: T
  label: string
  description?: string
  icon?: string
  /**
   * One or more Tailwind background-color classes rendered as small
   * leading dots before the label (in both the trigger and menu).
   * Use a single tone for options that map to one domain value
   * (e.g. Open -> status-open), or multiple tones for meta options
   * that span several. Single tones render as one 8px dot; multiple
   * tones render as a chip-stack of smaller 4px dots in sequence.
   */
  tones?: string[]
  /** Renders the option muted and non-selectable (click + keyboard
   * skip it). Use for choices the current actor isn't allowed to
   * pick while still showing them in context. */
  disabled?: boolean
}
