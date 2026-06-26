import type { IconName } from '@/components/common/icons';
import { ASSET_STATUSES, type AssetStatus } from '@nosdesk/core/types/asset';

export interface AssetStatusMeta {
  labelKey: string;
  colorClass: string;
  /** Solid swatch for filter-chip UI (matches warranty facet style). */
  swatchClass: string;
  icon: IconName;
}

export interface AssetStatusChipOption {
  value: AssetStatus;
  label: string;
  swatchClass: string;
}

const META: Record<AssetStatus, AssetStatusMeta> = {
  in_service: {
    labelKey: 'asset-status-in-service',
    colorClass: 'bg-status-success-muted text-status-success border-status-success/30',
    swatchClass: 'bg-emerald-500',
    icon: 'checkCircle',
  },
  in_stock: {
    labelKey: 'asset-status-in-stock',
    colorClass: 'bg-surface-alt text-secondary border-default',
    swatchClass: 'bg-zinc-400',
    icon: 'device',
  },
  in_repair: {
    labelKey: 'asset-status-in-repair',
    colorClass: 'bg-status-warning-muted text-status-warning border-status-warning/30',
    swatchClass: 'bg-amber-500',
    icon: 'warning',
  },
  on_loan: {
    labelKey: 'asset-status-on-loan',
    colorClass: 'bg-accent-muted text-accent border-accent/30',
    swatchClass: 'bg-blue-500',
    icon: 'team',
  },
  retired: {
    labelKey: 'asset-status-retired',
    colorClass: 'bg-surface-alt text-tertiary border-default',
    swatchClass: 'bg-zinc-400',
    icon: 'archive',
  },
  lost: {
    labelKey: 'asset-status-lost',
    colorClass: 'bg-status-error-muted text-status-error border-status-error/30',
    swatchClass: 'bg-rose-500',
    icon: 'xCircle',
  },
  disposed: {
    labelKey: 'asset-status-disposed',
    colorClass: 'bg-surface-alt text-tertiary border-default',
    swatchClass: 'bg-zinc-400',
    icon: 'trash',
  },
};

const FALLBACK: AssetStatusMeta = {
  labelKey: 'asset-status-unknown',
  colorClass: 'bg-surface-alt text-secondary border-default',
  swatchClass: 'bg-zinc-400',
  icon: 'circleDot',
};

export function metaForAssetStatus(status: string | null | undefined): AssetStatusMeta {
  if (status && status in META) {
    return META[status as AssetStatus];
  }
  return FALLBACK;
}

export function assetStatusLabel(
  t: (key: string) => string,
  status: string | null | undefined,
): string {
  return t(metaForAssetStatus(status).labelKey);
}

/** Chip-filter options for the assets list status facet. */
export function assetStatusChipOptions(
  t: (key: string) => string,
): AssetStatusChipOption[] {
  return ASSET_STATUSES.map((status) => {
    const meta = META[status];
    return { value: status, label: t(meta.labelKey), swatchClass: meta.swatchClass };
  });
}

/** Canonical sort order for status group-by buckets. */
export function assetStatusSortIndex(status: string): number {
  const idx = ASSET_STATUSES.indexOf(status as AssetStatus);
  return idx === -1 ? 999 : idx;
}
