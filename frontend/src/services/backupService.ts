import apiClient from './apiConfig';
import type {
  BackupJob,
  StartBackupExportRequest,
  ExecuteRestoreRequest,
  RestorePreview,
  RestoreResult,
} from '@/types/backup';

export const backupService = {
  /**
   * Start a new backup export job
   */
  async startExport(request: StartBackupExportRequest): Promise<BackupJob> {
    const response = await apiClient.post<BackupJob>('/admin/backup/export', request);
    return response.data;
  },

  /**
   * Get all backup/restore jobs
   */
  async getJobs(): Promise<BackupJob[]> {
    const response = await apiClient.get<BackupJob[]>('/admin/backup/jobs');
    return response.data;
  },

  /**
   * Get a specific job by ID
   */
  async getJob(id: string): Promise<BackupJob> {
    const response = await apiClient.get<BackupJob>(`/admin/backup/jobs/${id}`);
    return response.data;
  },

  /**
   * Delete a backup job and its associated file
   */
  async deleteJob(id: string): Promise<void> {
    await apiClient.delete(`/admin/backup/jobs/${id}`);
  },

  /**
   * Download a completed backup
   */
  downloadBackup(id: string): void {
    // Trigger browser download
    const url = `/api/admin/backup/download/${id}`;
    const link = document.createElement('a');
    link.href = url;
    link.download = '';
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
  },

  /**
   * Upload a backup file for restore
   */
  async uploadRestore(file: File): Promise<BackupJob> {
    const formData = new FormData();
    formData.append('file', file);

    const response = await apiClient.post<BackupJob>('/admin/backup/restore/upload', formData, {
      headers: {
        'Content-Type': 'multipart/form-data',
      },
    });
    return response.data;
  },

  /**
   * Preview what a restore would do
   */
  async getRestorePreview(id: string): Promise<RestorePreview> {
    const response = await apiClient.get<RestorePreview>(`/admin/backup/restore/${id}/preview`);
    return response.data;
  },

  /**
   * Execute a restore
   */
  async executeRestore(id: string, request: ExecuteRestoreRequest): Promise<RestoreResult> {
    const response = await apiClient.post<RestoreResult>(`/admin/backup/restore/${id}/execute`, request);
    return response.data;
  },

  /**
   * Poll a job until it's complete or failed
   */
  async pollJob(id: string, intervalMs = 2000, maxAttempts = 60): Promise<BackupJob> {
    for (let i = 0; i < maxAttempts; i++) {
      const job = await this.getJob(id);
      if (job.status === 'completed' || job.status === 'failed') {
        return job;
      }
      await new Promise(resolve => setTimeout(resolve, intervalMs));
    }
    throw new Error('Job polling timed out');
  },

  /**
   * Format file size for display
   */
  formatFileSize(bytes: number): string {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
  },
};

export default backupService;
