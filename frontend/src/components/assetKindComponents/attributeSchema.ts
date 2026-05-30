/**
 * Builder model + round-trip to the JSON Schema subset the
 * backend validator stores. Pure functions, no Vue imports, so
 * the parse/serialize logic is testable in isolation and stays
 * decoupled from the editor component.
 *
 * The builder works in `AttributeDef[]` ordered by appearance.
 * The backend stores `{ type: "object", properties: { ... },
 * required?: [...] }` — an unordered object plus a `required`
 * array — so the builder's order is preserved by iteration order
 * of the resulting `properties` object (JS preserves insertion
 * order for string keys, and the backend's validator doesn't
 * care about order at all).
 *
 * Unrecognised property shapes round-trip back as `{ kind: "raw",
 * json }` rather than being dropped, so an admin who lands here
 * from a hand-edited schema sees their custom keywords preserved
 * and can re-export the original via the "View JSON" toggle.
 */

/** Builder-level type taxonomy. Maps onto the validator's
 * type+format combinations via {@link defToProp}. */
export type AttributeKind =
  | 'text'
  | 'email'
  | 'url'
  | 'number'
  | 'decimal'
  | 'boolean'
  | 'date'
  | 'datetime'
  | 'select'
  | 'multi_select'
  | 'raw';

export const ATTRIBUTE_KINDS_ORDERED: Exclude<AttributeKind, 'raw'>[] = [
  'text',
  'email',
  'url',
  'number',
  'decimal',
  'boolean',
  'date',
  'datetime',
  'select',
  'multi_select',
];

export interface AttributeDef {
  /** Property name in the JSON schema. Constrained to a-z, 0-9,
   * underscore (matching the slug shape) to keep the JSON-side
   * key compatible across consumers. */
  name: string;
  kind: AttributeKind;
  required: boolean;
  description?: string;
  /** For `text`: optional maxLength. */
  maxLength?: number;
  /** For `text`: optional regex pattern. */
  pattern?: string;
  /** For `number` / `decimal`: optional bounds. */
  minimum?: number;
  maximum?: number;
  /** For `select` / `multi_select`: enum values. */
  enumValues?: string[];
  /** For `kind: "raw"`: the original JSON object for the property.
   * Lets unrecognised shapes round-trip without loss. */
  raw?: Record<string, unknown>;
}

export interface ParsedSchema {
  defs: AttributeDef[];
  /** Schema-level fields the builder doesn't surface as per-attr
   * config; preserved on round-trip. Empty in practice today
   * since the only top-level field the backend tracks is the
   * `required` array which we lift into `AttributeDef.required`. */
  extras: Record<string, unknown>;
}

const SAFE_NAME_RE = /^[a-z][a-z0-9_]*$/;

export function isValidAttributeName(name: string): boolean {
  return SAFE_NAME_RE.test(name);
}

/**
 * Parse a stored JSON Schema into the builder model. Unknown
 * property shapes degrade to `kind: "raw"` with the original
 * object preserved so the View-JSON toggle stays faithful.
 *
 * Throws if `schema` isn't a valid root object; the caller (the
 * editor view) catches and renders an inline error so the admin
 * sees the parse problem rather than a silent reset.
 */
export function parseSchema(schema: unknown): ParsedSchema {
  if (!schema || typeof schema !== 'object' || Array.isArray(schema)) {
    throw new Error('Schema must be a JSON object.');
  }
  const root = schema as Record<string, unknown>;
  if (root.type !== 'object') {
    throw new Error('Schema root must declare `type: "object"`.');
  }
  const properties = (root.properties ?? {}) as Record<string, unknown>;
  const required = new Set<string>(
    Array.isArray(root.required)
      ? (root.required as unknown[]).filter((v): v is string => typeof v === 'string')
      : [],
  );

  const defs: AttributeDef[] = Object.entries(properties).map(([name, raw]) => {
    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
      return { name, kind: 'raw', required: required.has(name), raw: { value: raw } };
    }
    const prop = raw as Record<string, unknown>;
    return propToDef(name, prop, required.has(name));
  });

  // Anything else on the root (e.g. an admin-added `description`)
  // is preserved verbatim so it survives a round-trip.
  const extras: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(root)) {
    if (k === 'type' || k === 'properties' || k === 'required') continue;
    extras[k] = v;
  }
  return { defs, extras };
}

function propToDef(
  name: string,
  prop: Record<string, unknown>,
  required: boolean,
): AttributeDef {
  const type = prop.type;
  const format = typeof prop.format === 'string' ? prop.format : undefined;

  // Multi-select: array of enum strings.
  if (type === 'array') {
    const items = prop.items as Record<string, unknown> | undefined;
    if (
      items &&
      items.type === 'string' &&
      Array.isArray(items.enum) &&
      items.enum.every((v) => typeof v === 'string')
    ) {
      return {
        name,
        kind: 'multi_select',
        required,
        description: stringOrUndefined(prop.description),
        enumValues: items.enum as string[],
      };
    }
    return { name, kind: 'raw', required, raw: prop };
  }

  if (type === 'boolean') {
    return {
      name,
      kind: 'boolean',
      required,
      description: stringOrUndefined(prop.description),
    };
  }

  if (type === 'integer' || type === 'number') {
    return {
      name,
      kind: type === 'integer' ? 'number' : 'decimal',
      required,
      description: stringOrUndefined(prop.description),
      minimum: numberOrUndefined(prop.minimum),
      maximum: numberOrUndefined(prop.maximum),
    };
  }

  if (type === 'string') {
    // Format-driven specialisations have their own builder kinds
    // so the dropdown stays scannable; the catch-all is `text`
    // with optional maxLength/pattern.
    if (format === 'email') {
      return {
        name,
        kind: 'email',
        required,
        description: stringOrUndefined(prop.description),
      };
    }
    if (format === 'uri') {
      return {
        name,
        kind: 'url',
        required,
        description: stringOrUndefined(prop.description),
      };
    }
    if (format === 'date') {
      return {
        name,
        kind: 'date',
        required,
        description: stringOrUndefined(prop.description),
      };
    }
    if (format === 'date-time') {
      return {
        name,
        kind: 'datetime',
        required,
        description: stringOrUndefined(prop.description),
      };
    }
    if (Array.isArray(prop.enum) && prop.enum.every((v) => typeof v === 'string')) {
      return {
        name,
        kind: 'select',
        required,
        description: stringOrUndefined(prop.description),
        enumValues: prop.enum as string[],
      };
    }
    return {
      name,
      kind: 'text',
      required,
      description: stringOrUndefined(prop.description),
      maxLength: numberOrUndefined(prop.maxLength),
      pattern: stringOrUndefined(prop.pattern),
    };
  }

  // Unrecognised: preserve verbatim.
  return { name, kind: 'raw', required, raw: prop };
}

/**
 * Serialise a builder model back to the JSON Schema subset the
 * backend stores. Always emits a stable shape: `type: "object"`,
 * `properties: { ... }`, and `required: [...]` when at least one
 * field is required.
 */
export function serializeSchema(parsed: ParsedSchema): Record<string, unknown> {
  const properties: Record<string, unknown> = {};
  const required: string[] = [];
  for (const def of parsed.defs) {
    if (!def.name) continue;
    properties[def.name] = defToProp(def);
    if (def.required) required.push(def.name);
  }
  const out: Record<string, unknown> = {
    type: 'object',
    properties,
    ...parsed.extras,
  };
  if (required.length > 0) out.required = required;
  return out;
}

function defToProp(def: AttributeDef): Record<string, unknown> {
  const base: Record<string, unknown> = {};
  if (def.description) base.description = def.description;

  switch (def.kind) {
    case 'text': {
      const out: Record<string, unknown> = { ...base, type: 'string' };
      if (def.maxLength != null) out.maxLength = def.maxLength;
      if (def.pattern) out.pattern = def.pattern;
      return out;
    }
    case 'email':
      return { ...base, type: 'string', format: 'email' };
    case 'url':
      return { ...base, type: 'string', format: 'uri' };
    case 'date':
      return { ...base, type: 'string', format: 'date' };
    case 'datetime':
      return { ...base, type: 'string', format: 'date-time' };
    case 'number': {
      const out: Record<string, unknown> = { ...base, type: 'integer' };
      if (def.minimum != null) out.minimum = def.minimum;
      if (def.maximum != null) out.maximum = def.maximum;
      return out;
    }
    case 'decimal': {
      const out: Record<string, unknown> = { ...base, type: 'number' };
      if (def.minimum != null) out.minimum = def.minimum;
      if (def.maximum != null) out.maximum = def.maximum;
      return out;
    }
    case 'boolean':
      return { ...base, type: 'boolean' };
    case 'select':
      return { ...base, type: 'string', enum: def.enumValues ?? [] };
    case 'multi_select':
      return {
        ...base,
        type: 'array',
        items: { type: 'string', enum: def.enumValues ?? [] },
      };
    case 'raw':
      return def.raw ?? {};
  }
}

function stringOrUndefined(v: unknown): string | undefined {
  return typeof v === 'string' ? v : undefined;
}
function numberOrUndefined(v: unknown): number | undefined {
  return typeof v === 'number' ? v : undefined;
}

/** Create a blank attribute of the given kind for "Add" button. */
export function blankAttribute(kind: Exclude<AttributeKind, 'raw'>): AttributeDef {
  const base: AttributeDef = { name: '', kind, required: false };
  if (kind === 'select' || kind === 'multi_select') base.enumValues = [];
  return base;
}
