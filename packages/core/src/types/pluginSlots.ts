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
export type SlotContextType = 'ticket' | 'asset' | 'documentationPage' | 'none';

/** Whether one plugin may contribute once or many times to a slot. */
export type SlotCardinality = 'one' | 'many';

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
    order: 100,
    status: 'stable',
    aliases: ['asset-info-panels'],
    description: 'Info panels on the asset view',
  },
  {
    name: 'settings.integrations.page',
    mechanism: 'panel',
    context: 'none',
    cardinality: 'many',
    order: 100,
    status: 'reserved',
    aliases: ['settings-integrations'],
    description: 'Pages under Settings > Integrations',
  },
  {
    name: 'dashboard.widget',
    mechanism: 'panel',
    context: 'none',
    cardinality: 'many',
    order: 100,
    status: 'reserved',
    aliases: [],
    description: 'Widgets on the dashboard',
  },
  {
    name: 'ticket.tab.panel',
    mechanism: 'panel',
    context: 'ticket',
    cardinality: 'many',
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
    order: 100,
    status: 'reserved',
    aliases: ['ticket-header-actions'],
    description: 'Actions in the ticket header menu',
  },
  {
    name: 'asset.header.action',
    mechanism: 'action',
    context: 'asset',
    cardinality: 'many',
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
    order: 100,
    status: 'reserved',
    aliases: ['navbar-items'],
    description: 'Links in the navigation sidebar',
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

// Back-compat shim for the old `PLUGIN_SLOTS` shape (`{ multiple, description }`),
// keyed by canonical name. Prefer `getSlot()` / `SLOT_REGISTRY` in new code.
export const PLUGIN_SLOTS = Object.fromEntries(
  SLOT_REGISTRY.map((s) => [s.name, { multiple: s.cardinality === 'many', description: s.description }]),
) as Record<CanonicalSlotName, { multiple: boolean; description: string }>;
