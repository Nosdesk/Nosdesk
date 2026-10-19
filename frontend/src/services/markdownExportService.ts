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

/**
 * Download the documentation export: every page the signed-in agent may
 * read, as Markdown files with their images and a manifest, in one zip the
 * server builds.
 */
export const downloadDocumentationExport = async (): Promise<void> => {
  const response = await apiClient.get<Blob>('/documentation/export.zip', {
    responseType: 'blob',
  });
  const name =
    fileNameFromDisposition(response.headers['content-disposition'] as string | undefined) ??
    `nosdesk-docs-${new Date().toISOString().split('T')[0]}.zip`;

  const url = URL.createObjectURL(response.data);
  const link = document.createElement('a');
  link.href = url;
  link.download = name;
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  URL.revokeObjectURL(url);
};
