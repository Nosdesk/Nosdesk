/** Compact group reference. Shared shape for both the directory groups an
 *  asset is synced into (`Asset.groups`) and the native asset groups it is
 *  classified under (`Asset.asset_groups`). */
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
  thumbnail_url?: string | null;
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
  /** Linked catalog model (asset_models row), or null/absent for a
   *  model-less asset. The manufacturer/model columns above hold the
   *  stamped snapshot regardless. */
  model_id?: number | null;
  // Joined enrichments from the REST endpoint
  primary_user?: {
    uuid: string;
    name: string;
    email: string;
    role: string;
    avatar_url?: string | null;
    avatar_thumb?: string | null;
  } | null;
  /** Directory-group memberships (Intune/Entra-synced or manual). */
  groups?: AssetGroup[];
  /** Native asset groups (workspace-local classification). */
  asset_groups?: AssetGroup[];
}

/** Canonical asset lifecycle states. Mirrors the backend
 *  `models::AssetStatus`; the list is fixed in code (statuses carry
 *  behaviour, so they are not per-workspace config). */
export type AssetStatus =
  | 'in_service'
  | 'in_stock'
  | 'on_order'
  | 'in_transit'
  | 'in_repair'
  | 'on_loan'
  | 'retired'
  | 'lost'
  | 'disposed';

export const ASSET_STATUSES: AssetStatus[] = [
  'in_service',
  'in_stock',
  'on_order',
  'in_transit',
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

/** A device loan: an asset in a borrower's custody for a span. Active while
 * `returned_at` is null; overdue while active and `due_back` is in the past. */
export interface AssetLoan {
  id: number;
  asset_id: number;
  borrower_user_uuid: string;
  loaned_at: string;
  due_back?: string | null;
  returned_at?: string | null;
  ticket_id?: number | null;
  notes?: string | null;
  actor_uuid?: string | null;
  returned_by_uuid?: string | null;
}

/** A manufacturer (make) in the asset model catalog. */
export interface Manufacturer {
  id: number;
  name: string;
  created_at: string;
  updated_at: string;
  created_by?: string | null;
}

/** An asset model ("device type"): a real make+model assets are stamped
 *  from. Belongs to a manufacturer, carries a kind + default specs. */
export interface AssetModel {
  id: number;
  manufacturer_id: number;
  name: string;
  kind: string;
  part_number?: string | null;
  default_attributes: Record<string, unknown>;
  notes?: string | null;
  created_at: string;
  updated_at: string;
  created_by?: string | null;
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
