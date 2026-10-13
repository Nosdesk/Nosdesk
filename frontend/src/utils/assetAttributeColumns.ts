/**
 * Inventory list columns for attributes declared in asset kinds'
 * `attribute_schema`. One column per property key across every kind; the
 * first kind (by sort order) to declare a key sets its label and type.
 * Column ids are `attr:<key>`, which saved views store like any other id.
 */
import type { AssetKind } from '@nosdesk/core/services/assetKindsService'

export const ATTRIBUTE_COLUMN_PREFIX = 'attr:'

export type AttributeValueType =
  | 'text'
  | 'number'
  | 'boolean'
  | 'date'
  | 'datetime'
  | 'user'
  | 'asset'
  | 'list'

export interface AttributeColumnDef {
  field: string
  key: string
  label: string
  valueType: AttributeValueType
}

export function isAttributeColumn(field: string): boolean {
  return field.startsWith(ATTRIBUTE_COLUMN_PREFIX)
}

export function attributeKeyOf(field: string): string {
  return field.slice(ATTRIBUTE_COLUMN_PREFIX.length)
}

function humanize(key: string): string {
  const words = key.replace(/_/g, ' ').trim()
  return words.charAt(0).toUpperCase() + words.slice(1)
}

function valueTypeOf(prop: Record<string, unknown>): AttributeValueType {
  switch (prop.type) {
    case 'boolean':
      return 'boolean'
    case 'integer':
    case 'number':
      return 'number'
    case 'array':
      return 'list'
  }
  switch (prop.format) {
    case 'date':
      return 'date'
    case 'date-time':
      return 'datetime'
    case 'user-uuid':
      return 'user'
    case 'asset-ref':
      return 'asset'
  }
  return 'text'
}

/** Column definitions for every attribute key, sorted by label. Keys in
 *  `exclude` already have a dedicated column. */
export function attributeColumnDefs(
  kinds: readonly AssetKind[],
  exclude: ReadonlySet<string> = new Set(),
): AttributeColumnDef[] {
  const byKey = new Map<string, AttributeColumnDef>()
  const ordered = [...kinds].sort((a, b) => a.sort_order - b.sort_order || a.id - b.id)
  for (const kind of ordered) {
    const properties = kind.attribute_schema?.properties
    if (!properties || typeof properties !== 'object') continue
    for (const [key, raw] of Object.entries(properties as Record<string, unknown>)) {
      if (byKey.has(key) || exclude.has(key) || !raw || typeof raw !== 'object') continue
      const prop = raw as Record<string, unknown>
      const title = typeof prop.title === 'string' && prop.title.trim() ? prop.title.trim() : null
      byKey.set(key, {
        field: `${ATTRIBUTE_COLUMN_PREFIX}${key}`,
        key,
        label: title ?? humanize(key),
        valueType: valueTypeOf(prop),
      })
    }
  }
  return [...byKey.values()].sort((a, b) => a.label.localeCompare(b.label))
}
