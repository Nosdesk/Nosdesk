import { ICON_REGISTRY, type IconName } from '@/components/common/icons'
import type { AssetKindCategory } from '@/services/assetKindsService'

/** Per-category fallback when a kind's stored `icon` isn't a real
 *  registry key. Every value here must be a valid IconName. */
const CATEGORY_FALLBACK: Record<AssetKindCategory, IconName> = {
  it: 'device',
  logical: 'key',
  physical: 'archive',
  bulk: 'database',
  generic: 'tag',
}

/**
 * Resolve an asset kind to a shared-registry icon name.
 *
 * A kind's `icon` is free text: admins can set anything, and several
 * builtins carry names like "laptop" / "vehicle" that aren't in our
 * registry. `<Icon>` throws on an unknown name, so we use the stored
 * value only when it's a real registry key and otherwise fall back to
 * a per-category default (then to `device`). Guarantees a safe name.
 */
export function kindIconName(
  kind: { icon?: string | null; category?: AssetKindCategory } | null | undefined,
): IconName {
  const raw = kind?.icon
  if (raw && raw in ICON_REGISTRY) return raw as IconName
  const cat = kind?.category
  if (cat && cat in CATEGORY_FALLBACK) return CATEGORY_FALLBACK[cat]
  return 'device'
}
