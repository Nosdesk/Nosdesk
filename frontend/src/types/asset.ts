export interface AssetGroup {
  id: number;
  uuid: string;
  name: string;
  color?: string | null;
}

export interface AssetMedia {
  id: number;
  asset_id: number;
  url: string;
  name: string;
  file_size?: number | null;
  mime_type?: string | null;
  kind: string;
  sort_order: number;
  caption?: string | null;
  uploaded_by?: string | null;
  created_at: string;
}

/** Asset row as the REST + sync surfaces ship it. IT-flavoured
 *  fields (hostname, OS, warranty, Microsoft Graph IDs etc.)
 *  live inside `attributes` after Pass B. The legacy top-level
 *  `intune_device_id` / `entra_device_id` / `warranty_status`
 *  / etc. are gone; read them via `attributes['hostname']`. */
export interface Asset {
  id: number;
  name: string;
  kind: string;
  attributes: Record<string, unknown>;
  serial_number: string;
  model: string;
  manufacturer?: string | null;
  location?: string | null;
  /** Lifecycle state. One of `AssetStatus`; defaults to
   *  `in_service`. Changed via the lifecycle transition endpoint,
   *  not a plain asset edit. */
  status: string;
  primary_user_uuid?: string | null;
  created_at: string;
  updated_at: string;
  purchase_date?: string | null;
  asset_tag?: string | null;
  /** BigDecimal-as-string. NUMERIC(12,3) on the backend. */
  quantity?: string | null;
  unit?: string | null;
  /** `'intune'` / `'entra'` when sync-owned; null otherwise. */
  external_sync_source?: string | null;
  /** BigDecimal-as-string. Optional low-stock threshold; when
   *  set and `quantity` is at or below this value, the asset is
   *  rendered as low-stock in the UI. */
  low_stock_threshold?: string | null;
  is_editable: boolean;
  // Joined enrichments from the REST endpoint
  primary_user?: {
    uuid: string;
    name: string;
    email: string;
    role: string;
    avatar_url?: string | null;
    avatar_thumb?: string | null;
  } | null;
  groups?: AssetGroup[];
}

/** Canonical asset lifecycle states. Mirrors the backend
 *  `models::AssetStatus`; the list is fixed in code (statuses carry
 *  behaviour, so they are not per-workspace config). */
export type AssetStatus =
  | 'in_service'
  | 'in_stock'
  | 'in_repair'
  | 'on_loan'
  | 'retired'
  | 'lost'
  | 'disposed';

export const ASSET_STATUSES: AssetStatus[] = [
  'in_service',
  'in_stock',
  'in_repair',
  'on_loan',
  'retired',
  'lost',
  'disposed',
];

/** One row of an asset's append-only lifecycle log. `metadata`
 *  carries state-specific fields (repair vendor/RMA/offsite, loan
 *  recipient/due-back) so new workflows need no schema change. */
export interface AssetLifecycleEvent {
  id: number;
  asset_id: number;
  from_status?: string | null;
  to_status: string;
  reason?: string | null;
  ticket_id?: number | null;
  metadata: Record<string, unknown>;
  actor_uuid?: string | null;
  occurred_at: string;
}

/** Wire shape for POST /assets (create) and the wider parts
 *  of PUT /assets/:id. IT-flavoured fields go in `attributes`
 *  now; this DTO mirrors the universal columns plus the
 *  kind/attributes pair. */
export interface AssetFormData {
  name: string;
  serial_number?: string;
  model?: string;
  manufacturer?: string | null;
  location?: string | null;
  primary_user_uuid?: string | null;
  purchase_date?: string | null;
  asset_tag?: string | null;
  kind?: string;
  attributes?: Record<string, unknown>;
  quantity?: string | null;
  unit?: string | null;
  low_stock_threshold?: string | null;
}
