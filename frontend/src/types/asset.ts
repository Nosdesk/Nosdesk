export interface AssetGroup {
  id: number;
  uuid: string;
  name: string;
  color?: string | null;
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
}
