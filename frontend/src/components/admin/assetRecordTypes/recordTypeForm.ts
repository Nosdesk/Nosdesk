/**
 * Pure helpers for the record type editor: the field order round trip,
 * catalogue names for built-ins, outcome keys, and the diff a save sends.
 * No Vue imports, so the specs reach them directly.
 */
import type {
  AssetRecordCoreField,
  AssetRecordCoreFieldMode,
  AssetRecordOutcome,
  AssetRecordOutcomeCategory,
  AssetRecordType,
} from '@nosdesk/core/types/asset';

export const OUTCOME_CATEGORIES: AssetRecordOutcomeCategory[] = [
  'done',
  'not_done',
  'no_fault',
  'replaced',
];

export const CORE_FIELDS: AssetRecordCoreField[] = [
  'summary',
  'outcome',
  'vendor',
  'reference',
  'cost',
  'ticket',
  'holder',
  'location',
];

export const CORE_FIELD_MODES: AssetRecordCoreFieldMode[] = ['hidden', 'optional', 'required'];

/** Icons a type can carry; the built-ins use the first four. */
export const RECORD_TYPE_ICONS = [
  'settings',
  'check',
  'calendar',
  'upload',
  'clipboardCheck',
  'shieldCheck',
  'refresh',
  'cycle',
  'clock',
  'history',
  'lightning',
  'flag',
  'warning',
  'eye',
  'key',
  'lock',
  'document',
  'tag',
  'device',
  'laptop',
  'phone',
  'monitor',
  'server',
  'network',
  'vehicle',
  'equipment',
  'consumable',
  'paint',
] as const;

/** A registry icon's name as words, for its button's label: the registry
 *  has no translated names, so `clipboardCheck` reads "Clipboard check". */
export function iconLabel(name: string): string {
  const words = name.replace(/([a-z0-9])([A-Z])/g, '$1 $2').toLowerCase();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

type Translate = (key: string, args?: Record<string, string | number>) => string;

/** A type's name: its own label, or the catalogue name of a built-in nobody
 *  renamed, which follows the reader's language. */
export function recordTypeName(t: Translate, type: Pick<AssetRecordType, 'slug' | 'label'>): string {
  return type.label ?? t(`asset-record-type-${type.slug}`);
}

/** The catalogue name of a built-in outcome. */
export function catalogueOutcomeName(t: Translate, typeSlug: string, key: string): string {
  return t(`asset-record-outcome-${typeSlug}-${key}`);
}

/** Lower case letters, digits and underscores, starting with a letter: the
 *  shape the server takes for slugs and outcome keys. */
export function keyFrom(text: string): string {
  return text
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9_\s-]+/g, '')
    .replace(/[\s-]+/g, '_')
    .replace(/^[^a-z]+/, '')
    .slice(0, 64);
}

/** `base`, or `base_2`, `base_3` and on while `taken` has it. */
export function uniqueKey(base: string, taken: Iterable<string>): string {
  const used = new Set(taken);
  const root = base || 'outcome';
  if (!used.has(root)) return root;
  let n = 2;
  while (used.has(`${root}_${n}`)) n += 1;
  return `${root}_${n}`;
}

type Schema = Record<string, unknown>;

function propertiesOf(schema: Schema): Record<string, unknown> {
  const props = schema.properties;
  return props && typeof props === 'object' && !Array.isArray(props)
    ? (props as Record<string, unknown>)
    : {};
}

/**
 * The schema with `properties` rebuilt in `order`. The server stores the
 * schema as jsonb, which sorts object keys, so the display order lives in
 * `field_order`; the field builder reads order from key insertion. Keys the
 * order leaves out keep their place after the ordered ones.
 */
export function inFieldOrder(schema: Schema, order: string[]): Schema {
  const props = propertiesOf(schema);
  const ordered: Record<string, unknown> = {};
  for (const key of order) {
    if (key in props) ordered[key] = props[key];
  }
  for (const [key, value] of Object.entries(props)) {
    if (!(key in ordered)) ordered[key] = value;
  }
  return { ...schema, properties: ordered };
}

/** The order to save: the builder's property order. */
export function fieldOrderOf(schema: Schema): string[] {
  return Object.keys(propertiesOf(schema));
}

/** JSON with object keys sorted, so two schemas compare equal whatever
 *  order their keys were written in. */
export function canonicalJson(value: unknown): string {
  const sort = (v: unknown): unknown => {
    if (Array.isArray(v)) return v.map(sort);
    if (v && typeof v === 'object') {
      return Object.fromEntries(
        Object.keys(v as Record<string, unknown>)
          .sort()
          .map((k) => [k, sort((v as Record<string, unknown>)[k])]),
      );
    }
    return v;
  };
  return JSON.stringify(sort(value));
}

/** An outcome row as the editor holds it. `label` is '' on a built-in
 *  outcome that reads its catalogue name. `isNew` rows get a key from their
 *  name; saved keys are on records and never change. */
export interface OutcomeRow {
  key: string;
  category: AssetRecordOutcomeCategory;
  label: string;
  isNew: boolean;
}

export function outcomeRows(outcomes: AssetRecordOutcome[]): OutcomeRow[] {
  return outcomes.map((o) => ({
    key: o.key,
    category: o.category,
    label: o.label ?? '',
    isNew: false,
  }));
}

/** The outcomes to send: a blank name is the catalogue's (null). */
export function outcomesFromRows(rows: OutcomeRow[]): AssetRecordOutcome[] {
  return rows.map((r) => {
    const label = r.label.trim();
    return label
      ? { key: r.key, category: r.category, label }
      : { key: r.key, category: r.category, label: null };
  });
}

/** Outcomes compared as the server stores them. */
export function sameOutcomes(a: AssetRecordOutcome[], b: AssetRecordOutcome[]): boolean {
  const norm = (list: AssetRecordOutcome[]) =>
    list.map((o) => ({ key: o.key, category: o.category, label: o.label?.trim() || null }));
  return JSON.stringify(norm(a)) === JSON.stringify(norm(b));
}

/** Core field modes with the default (optional) left out, as stored. */
export function compactCoreFields(
  modes: Partial<Record<AssetRecordCoreField, AssetRecordCoreFieldMode>>,
): Partial<Record<AssetRecordCoreField, AssetRecordCoreFieldMode>> {
  const out: Partial<Record<AssetRecordCoreField, AssetRecordCoreFieldMode>> = {};
  for (const field of CORE_FIELDS) {
    const mode = modes[field];
    if (mode && mode !== 'optional') out[field] = mode;
  }
  return out;
}

/** One record a refused change would no longer fit: the record (`id`), the
 *  asset it is on, and why. */
export interface RecordsConflictSample {
  id: number;
  asset_id: number;
  asset_name: string;
  asset_tag: string | null;
  error: string;
}

/** A save the server refused because records of the type would no longer
 *  fit: how many, and a few of them. */
export interface RecordsConflict {
  invalidCount: number;
  sample: RecordsConflictSample[];
}

export function recordsConflict(body: Record<string, unknown> | undefined): RecordsConflict | null {
  if (!body || body.code !== 'RECORDS_WOULD_NOT_FIT') return null;
  const count = body.invalid_count;
  if (typeof count !== 'number') return null;
  const sample = Array.isArray(body.sample) ? (body.sample as RecordsConflict['sample']) : [];
  return { invalidCount: count, sample };
}
