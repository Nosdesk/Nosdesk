// src/constants/ticketOptions.ts
//
// Status + priority option lists for the ticket pickers. Each entry
// carries a Fluent key (`labelKey`) that consumers resolve at render
// time with `useFluent().$t()` or `translate()` from `@/i18n`, so a
// locale switch re-labels the dropdowns without re-evaluating this
// module. The English literal stays in a sibling fallback map so
// pre-bootstrap call sites (tests, SSR) still get readable text.
export type TicketPriority = "none" | "low" | "medium" | "high" | "urgent";

export interface SelectOption<T extends string> {
  value: T;
  /** Fluent key. Consumers resolve via `$t(opt.labelKey)`. */
  labelKey: string;
}

/** Every priority the backend stores, most severe first. */
export const PRIORITY_OPTIONS: SelectOption<TicketPriority>[] = [
  { value: "urgent", labelKey: "priority-urgent" },
  { value: "high", labelKey: "priority-high" },
  { value: "medium", labelKey: "priority-medium" },
  { value: "low", labelKey: "priority-low" },
  { value: "none", labelKey: "priority-none" },
];

/** Severity, 0 (none) to 4 (urgent), as the backend ranks it. */
export const PRIORITY_RANK: Record<TicketPriority, number> = {
  none: 0,
  low: 1,
  medium: 2,
  high: 3,
  urgent: 4,
};

/** A priority's severity; anything that isn't one ranks as none. */
export function priorityRank(priority: string | null | undefined): number {
  return PRIORITY_RANK[priority as TicketPriority] ?? 0;
}

/** Compare two priorities by severity, least severe first. */
export function comparePriority(
  a: string | null | undefined,
  b: string | null | undefined,
): number {
  return priorityRank(a) - priorityRank(b);
}
