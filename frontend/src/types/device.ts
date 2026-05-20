export interface DeviceGroup {
  id: number;
  uuid: string;
  name: string;
  color?: string | null;
}

export interface Device {
  id: number;
  name: string;
  hostname: string;
  serial_number: string;
  model: string;
  warranty_status: string;
  manufacturer?: string | null;
  operating_system?: string | null;
  os_version?: string | null;
  primary_user_uuid?: string | null;
  intune_device_id?: string | null;
  entra_device_id?: string | null;
  created_at: string;
  updated_at: string;
  last_sync_time?: string | null;
  warranty_start_date?: string | null;
  warranty_end_date?: string | null;
  purchase_date?: string | null;
  asset_tag?: string | null;
  is_editable: boolean;
  /** Asset-kind discriminator. Slug from the asset_kinds
   *  registry. Defaults to `'device'` for IT-desk assets. */
  kind?: string;
  /** Kind-specific attributes validated server-side against
   *  the kind's attribute_schema. */
  attributes?: Record<string, unknown>;
  /** Optional bulk-material quantity; paired with `unit`. */
  quantity?: string | number | null;
  unit?: string | null;
  // Computed/joined fields from API
  primary_user?: {
    uuid: string;
    name: string;
    email: string;
    role: string;
    avatar_url?: string | null;
    avatar_thumb?: string | null;
  } | null;
  groups?: DeviceGroup[];
  // Legacy fields for backward compatibility
  type?: string;
  lastSeen?: string;
  status?: string;
  specs?: {
    cpu?: string;
    memory?: string;
    storage?: string;
    os?: string;
  };
  assignedTo?: string | null;
}

export interface DeviceFormData {
  name: string;
  hostname: string;
  serial_number: string;
  model: string;
  warranty_status: string;
  manufacturer?: string;
  primary_user_uuid?: string | null;
  intune_device_id?: string;
  entra_device_id?: string;
  warranty_start_date?: string | null;
  warranty_end_date?: string | null;
  purchase_date?: string | null;
  asset_tag?: string | null;
  type?: string;
  kind?: string;
  attributes?: Record<string, unknown>;
  quantity?: string | number | null;
  unit?: string | null;
}
