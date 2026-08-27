import apiClient from '@nosdesk/core/apiClient';

/** A self-serve workspace data export job (mirrors the backend `job_view`). */
export interface WorkspaceExportJob {
  id: string;
  status: 'pending' | 'processing' | 'completed' | 'failed';
  file_size: number | null;
  error_message: string | null;
  created_at: string;
  completed_at: string | null;
  expires_at: string | null;
  download_available: boolean;
}

export const workspaceExportService = {
  /** Start an export of the current workspace. Owner-only (enforced server-side). */
  async requestExport(password?: string): Promise<WorkspaceExportJob> {
    const res = await apiClient.post<WorkspaceExportJob>('/workspace/export', {
      password: password || undefined,
    });
    return res.data;
  },

  /** The workspace's most recent export, or `null` if there is none. */
  async getLatest(): Promise<WorkspaceExportJob | null> {
    const res = await apiClient.get<WorkspaceExportJob | null>('/workspace/export');
    return res.data ?? null;
  },

  /** Fetch one export job's status. */
  async getExport(id: string): Promise<WorkspaceExportJob> {
    const res = await apiClient.get<WorkspaceExportJob>(`/workspace/export/${id}`);
    return res.data;
  },

  /** Poll until the export completes or fails. */
  async pollExport(id: string, intervalMs = 2500, maxAttempts = 160): Promise<WorkspaceExportJob> {
    for (let i = 0; i < maxAttempts; i++) {
      const job = await this.getExport(id);
      if (job.status === 'completed' || job.status === 'failed') return job;
      await new Promise((resolve) => setTimeout(resolve, intervalMs));
    }
    throw new Error('Export polling timed out');
  },

  /** Trigger the browser download of a completed export's artifact. */
  downloadExport(id: string): void {
    const link = document.createElement('a');
    link.href = `/api/workspace/export/${id}/download`;
    link.download = '';
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
  },
};

export default workspaceExportService;
