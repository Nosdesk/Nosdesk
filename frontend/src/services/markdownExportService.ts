import apiClient from '@nosdesk/core/apiClient';

/** The server's name for the download, from a `Content-Disposition` header. */
export function fileNameFromDisposition(header: string | null | undefined): string | null {
  if (!header) return null;
  const encoded = /filename\*=UTF-8''([^;]+)/i.exec(header);
  if (encoded) {
    try {
      return decodeURIComponent(encoded[1].trim());
    } catch {
      // Fall through to the plain parameter.
    }
  }
  const plain = /filename="?([^";]+)"?/i.exec(header);
  return plain ? plain[1].trim() : null;
}

/** Why an export failed, for the caller to word. */
export type DocumentationExportFailure = 'forbidden' | 'failed';

export class DocumentationExportError extends Error {
  constructor(
    readonly reason: DocumentationExportFailure,
    message: string,
  ) {
    super(message);
    this.name = 'DocumentationExportError';
  }
}

/** The name to save under when the server sends none. */
export function fallbackExportName(now: Date = new Date()): string {
  return `documentation-${now.toISOString().split('T')[0]}.zip`;
}

/**
 * Download the documentation export: the workspace's Library and Drafts as
 * Markdown files with their images and a manifest, in one zip the server
 * builds. Workspace admins and owners only. Resolves with the file name it
 * saved under; rejects with a {@link DocumentationExportError}.
 */
export const downloadDocumentationExport = async (): Promise<string> => {
  let response;
  try {
    response = await apiClient.get<Blob>('/documentation/export.zip', {
      responseType: 'blob',
    });
  } catch (error) {
    const status = (error as { response?: { status?: number } }).response?.status;
    throw new DocumentationExportError(
      status === 403 ? 'forbidden' : 'failed',
      error instanceof Error ? error.message : String(error),
    );
  }
  const name =
    fileNameFromDisposition(response.headers['content-disposition'] as string | undefined) ??
    fallbackExportName();

  const url = URL.createObjectURL(response.data);
  const link = document.createElement('a');
  link.href = url;
  link.download = name;
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  URL.revokeObjectURL(url);
  return name;
};
