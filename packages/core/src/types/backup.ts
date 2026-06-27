export interface BackupJob {
  id: string;
  job_type: 'export' | 'restore';
  status: 'pending' | 'processing' | 'completed' | 'failed';
  include_sensitive: boolean;
  file_path?: string;
  file_size?: number;
  error_message?: string;
  created_by?: string;
  created_at: string;
  completed_at?: string;
}

export interface StartBackupExportRequest {
  include_sensitive: boolean;
  password?: string;
}

export interface ExecuteRestoreRequest {
  password?: string;
}

export interface TableManifest {
  count: number;
}

export interface FilesManifest {
  total_count: number;
  total_size_bytes: number;
}

export interface EncryptionManifest {
  algorithm: string;
  kdf: string;
  salt: string;
  nonce: string;
}

export interface BackupManifest {
  version: string;
  created_at: string;
  nosdesk_version: string;
  include_sensitive: boolean;
  tables: Record<string, TableManifest>;
  files: FilesManifest;
  encryption?: EncryptionManifest;
}

export interface RestorePreview {
  manifest: BackupManifest;
  has_encrypted_sensitive: boolean;
  warnings: string[];
}

export interface RestoreResult {
  success: boolean;
  files_restored: number;
  message: string;
}
