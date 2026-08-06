// =============================================================================
// Plugin UI extension points — single source of truth
// =============================================================================
//
// This module is the ONE canonical declaration of every plugin UI extension
// point. Both toolchains read from here:
//
//   * Frontend/TS: imports `SLOT_REGISTRY` + the helpers/types below directly.
//   * Backend/Rust: reads `backend/src/services/plugins/plugin_slots.generated.json`,
//     which is generated from this file by `pnpm --filter @nosdesk/core build:slots`
//     and drift-checked in CI (mirrors the runtime.js pattern).
//
// Adding or changing a slot is a one-place edit here, then `build:slots` to
// regenerate the JSON. There is no hand-synced Rust allowlist any more.
//
// Two mechanisms (see docs/plugin-ui-slots-design.md):
//   * `panel`  — an iframe surface the plugin fills, wrapped in host chrome.
//   * `action` — declarative metadata the host renders as native chrome
//                (menu item, button, nav link) which then activates the plugin.
//
// Taxonomy is dotted `<entity>.<region>.<kind>` so the name self-documents its
// placement and the host can pattern-match a family. Old flat kebab-case names
// live on as `aliases` for graceful back-compat; a manifest may target either.

/** How the host renders a contribution to this slot. */
export type SlotMechanism = 'panel' | 'action';

/** The single host-provided context object a mount hands to the plugin. */
export type SlotContextType =
  | 'ticket'
  | 'asset'
  | 'user'
  | 'address'
  | 'documentationPage'
  | 'none';

/** Whether one plugin may contribute once or many times to a slot. */
export type SlotCardinality = 'one' | 'many';

/**
 * The chrome the HOST draws around a `panel` contribution.
 *
 *   * `card` — the host wraps the iframe in the app's `SectionCard`: same
 *     radius, border, surface and compact header pill as every built-in card,
 *     titled from the manifest `label`. The plugin fills the body only, so it
 *     must NOT draw its own outer card (that reads as a double border).
 *   * `none` — the host mounts the bare frame. For contributions whose host
 *     already supplies chrome (a widget shell, a page heading) or that sit
 *     inline inside another card, where a second card would be wrong.
 *
 * This is the slot's DEFAULT; a manifest component may override it (see
 * `chrome` on the component config) for genuinely full-bleed content.
 *
 * Meaningless on `action` slots, which the host renders as native chrome
 * already; they are declared `none` and validation rejects an override.
 */
export type SlotChrome = 'card' | 'none';

/**
 * Lifecycle of a slot in the taxonomy.
 *   * `stable`       — has a live mount point; renders today.
 *   * `reserved`     — declared for authors to target, no mount yet (silent
 *                      no-op until one lands, like a VS Code contribution point).
 *   * `experimental` — mounted but the contract may still change.
 */
export type SlotStatus = 'stable' | 'reserved' | 'experimental';

export interface SlotDef {
  /** Canonical dotted identifier. */
  readonly name: string;
  readonly mechanism: SlotMechanism;
  readonly context: SlotContextType;
  readonly cardinality: SlotCardinality;
  /** Host chrome drawn around a contribution, unless the component overrides. */
  readonly chrome: SlotChrome;
  /** Default sort order within the slot; host tiebreaks by install order. */
  readonly order: number;
  readonly status: SlotStatus;
  /** Legacy flat names still accepted at validation/registration time. */
  readonly aliases: readonly string[];
  readonly description: string;
}

// The registry. `as const satisfies` keeps literal `name`/`aliases` types (so
// the derived unions below are precise) while checking each row's shape.
export const SLOT_REGISTRY = [
  {
    name: 'ticket.sidebar.panel',
    mechanism: 'panel',
    context: 'ticket',
    cardinality: 'many',
    chrome: 'card',
    order: 100,
    status: 'stable',
    aliases: ['ticket-sidebar'],
    description: 'Panels in the ticket sidebar',
  },
  {
    name: 'asset.info.panel',
    mechanism: 'panel',
    context: 'asset',
    cardinality: 'many',
    chrome: 'card',
    order: 100,
    status: 'stable',
    aliases: ['asset-info-panels'],
    description: 'Info panels on the asset view',
  },
  {
    name: 'user.sidebar.panel',
    mechanism: 'panel',
    context: 'user',
    cardinality: 'many',
    chrome: 'card',
    order: 100,
    status: 'stable',
    aliases: [],
    description: 'Info panels on the user profile view',
  },
  {
    name: 'user.address.panel',
    mechanism: 'panel',
    context: 'address',
    cardinality: 'many',
    // Inline enrichment under an address row that is already inside the
    // addresses card; a card here would nest one card in another.
    chrome: 'none',
    order: 100,
    status: 'stable',
    aliases: [],
    description: "Enrichment panel per address row on a user's addresses card",
  },
  {
    name: 'settings.integrations.page',
    mechanism: 'panel',
    context: 'none',
    cardinality: 'many',
    // The detail view already frames this with its own section heading.
    chrome: 'none',
    order: 100,
    status: 'stable',
    aliases: ['settings-integrations'],
    description: "A plugin's configuration page, shown on its admin detail view",
  },
  {
    name: 'dashboard.widget',
    mechanism: 'panel',
    context: 'none',
    cardinality: 'many',
    // `PluginDashboardWidget` already mounts these inside
    // `DashboardWidgetShell`, which supplies the title and body metrics.
    chrome: 'none',
    order: 100,
    status: 'stable',
    aliases: [],
    description: 'Widgets on the dashboard (opt-in via the Add widget picker)',
  },
  {
    name: 'ticket.tab.panel',
    mechanism: 'panel',
    context: 'ticket',
    cardinality: 'many',
    // A tab body is already inside the tab frame.
    chrome: 'none',
    order: 100,
    status: 'reserved',
    aliases: ['ticket-tabs'],
    description: 'Tabs on the ticket view',
  },
  {
    name: 'document.sidebar.panel',
    mechanism: 'panel',
    context: 'documentationPage',
    cardinality: 'many',
    chrome: 'card',
    order: 100,
    status: 'reserved',
    aliases: ['document-sidebar'],
    description: 'Panels in the document sidebar',
  },
  {
    name: 'ticket.header.action',
    mechanism: 'action',
    context: 'ticket',
    cardinality: 'many',
    chrome: 'none',
    order: 100,
    status: 'stable',
    aliases: ['ticket-header-actions'],
    description: 'Actions in the ticket header menu (open the component in a modal)',
  },
  {
    name: 'ticket.bulk.action',
    mechanism: 'action',
    context: 'ticket',
    cardinality: 'many',
    chrome: 'none',
    order: 100,
    status: 'stable',
    aliases: [],
    description: 'Actions on a ticket selection (open the component in a modal)',
  },
  {
    name: 'asset.header.action',
    mechanism: 'action',
    context: 'asset',
    cardinality: 'many',
    chrome: 'none',
    order: 100,
    status: 'reserved',
    aliases: ['asset-header-actions'],
    description: 'Actions in the asset header menu',
  },
  {
    name: 'document.toolbar.action',
    mechanism: 'action',
    context: 'documentationPage',
    cardinality: 'many',
    chrome: 'none',
    order: 100,
    status: 'reserved',
    aliases: ['document-toolbar'],
    description: 'Actions in the document toolbar menu',
  },
  {
    name: 'nav.item',
    mechanism: 'action',
    context: 'none',
    cardinality: 'many',
    chrome: 'none',
    order: 100,
    status: 'stable',
    aliases: ['navbar-items'],
    description: 'A nav-sidebar link to a full-page plugin surface',
  },
] as const satisfies readonly SlotDef[];

/** A canonical dotted slot name. */
export type CanonicalSlotName = (typeof SLOT_REGISTRY)[number]['name'];
/** Any legacy flat alias still accepted. */
export type LegacySlotName = (typeof SLOT_REGISTRY)[number]['aliases'][number];
/** Either form — what a manifest or call site may reference. */
export type AnySlotName = CanonicalSlotName | LegacySlotName;

const BY_NAME = new Map<string, SlotDef>();
for (const def of SLOT_REGISTRY) {
  BY_NAME.set(def.name, def);
  for (const alias of def.aliases) BY_NAME.set(alias, def);
}

/** Resolve a canonical or alias name to its definition, else undefined. */
export function getSlot(name: string): SlotDef | undefined {
  return BY_NAME.get(name);
}

/** True if `name` is a known canonical name or alias. */
export function isKnownSlot(name: string): boolean {
  return BY_NAME.has(name);
}

/** Canonical name for a canonical-or-alias input, else undefined. */
export function canonicalSlotName(name: string): CanonicalSlotName | undefined {
  return BY_NAME.get(name)?.name as CanonicalSlotName | undefined;
}

/** All canonical names, in registry order. */
export const SLOT_NAMES = SLOT_REGISTRY.map((s) => s.name) as readonly CanonicalSlotName[];
