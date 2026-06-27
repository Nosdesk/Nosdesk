// src/constants/ticketOptions.ts
//
// Status + priority option lists for the ticket pickers. Each entry
// carries a Fluent key (`labelKey`) that consumers resolve at render
// time with `useFluent().$t()` or `translate()` from `@/i18n`, so a
// locale switch re-labels the dropdowns without re-evaluating this
// module. The English literal stays in a sibling fallback map so
// pre-bootstrap call sites (tests, SSR) still get readable text.
export type TicketPriority = "low" | "medium" | "high";

export interface SelectOption<T extends string> {
  value: T;
  /** Fluent key. Consumers resolve via `$t(opt.labelKey)`. */
  labelKey: string;
}

export const PRIORITY_OPTIONS: SelectOption<TicketPriority>[] = [
  { value: "low", labelKey: "priority-low" },
  { value: "medium", labelKey: "priority-medium" },
  { value: "high", labelKey: "priority-high" },
];
