export interface Device {
  id: number;
  name: string;
  hostname: string;
  serial_number: string;
  model: string;
  warranty_status: string;
  manufacturer?: string | null;
  primary_user_uuid?: string | null;
  intune_device_id?: string | null;
  entra_device_id?: string | null;
  created_at: string;
  updated_at: string;
  // Computed/joined fields from API
  primary_user?: {
    uuid: string;
    name: string;
    email: string;
    role: string;
    avatar_url?: string | null;
    avatar_thumb?: string | null;
  } | null;
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
  type?: string;
} 