/**
 * Editor Image Upload Service
 *
 * Uploads images pasted or dropped into a collaborative editor and returns a
 * URL reference for the `image` node. The node's payload stays a short URL:
 * base64 dataURIs in the Yjs document are the failure mode this whole
 * subsystem exists to avoid.
 *
 * The upload target is derived from the collab docId the editor is already
 * bound to (`ws-{workspace_uuid}_{kind}-{resource_uuid}`), so documentation
 * pages and collection descriptions work the same way tickets do. There is no
 * fallback to the generic `/upload` staging endpoint: that stored into `temp/`
 * and handed back an unreachable `/uploads/...` URL, which is what made pasted
 * images look broken on documentation pages.
 */

import apiClient from '@nosdesk/core/apiClient';
import { parseCollabDocId } from '@nosdesk/core/utils/collabDocId';
import uploadService from '@/services/uploadService';
import { logger } from '@nosdesk/core/utils/logger';

export type EditorImageErrorCode =
  | 'no-target'
  | 'invalid-file'
  | 'upload-failed'
  /** Uploaded fine, but the spot it was pasted at no longer exists. */
  | 'anchor-lost';

/**
 * Typed so the editor can pick the right message per failure mode instead of
 * swallowing every failure into one console line.
 */
export class EditorImageUploadError extends Error {
  constructor(
    readonly code: EditorImageErrorCode,
    message: string,
    readonly cause?: unknown
  ) {
    super(message);
    this.name = 'EditorImageUploadError';
  }
}

export interface EditorImageUploadResult {
  url: string;
  name: string;
  size?: number;
}

export interface EditorImageUploadOptions {
  /** Workspace-namespaced collab doc id the editor is bound to. */
  docId: string;
  onProgress?: (message: string) => void;
}

interface UploadedEditorImage {
  url: string;
  name: string;
  size?: number;
}

/**
 * Upload an image for use in the collaborative editor. Resolves to the final
 * servable URL, already in `/api/files/collab/...` form; callers insert it as
 * the image node's `src` verbatim.
 */
export async function uploadEditorImage(
  file: File,
  options: EditorImageUploadOptions
): Promise<EditorImageUploadResult> {
  const { docId, onProgress } = options;

  // A docId that does not parse means there is no document to attach to: the
  // brand-new-page wizard's `documentation-new` sentinel, a legacy bare id, or
  // an unresolved workspace. Fail loudly rather than staging the file
  // somewhere it can never be served from.
  if (!parseCollabDocId(docId)) {
    throw new EditorImageUploadError(
      'no-target',
      `Cannot upload an image for an unsaved or unparseable document (got "${docId}")`
    );
  }

  const validation = uploadService.validateFile(file, {
    maxSizeMB: 10,
    allowedTypes: ['image/*'],
  });
  if (!validation.valid) {
    throw new EditorImageUploadError('invalid-file', validation.error || 'Invalid file');
  }

  const processedFile = await uploadService.convertHeicToJpeg(file, onProgress);

  const formData = new FormData();
  // Explicit filename third argument: HEIC conversion renames the file.
  formData.append('files', processedFile, processedFile.name);

  onProgress?.('Uploading image...');

  try {
    // Through the shared axios client, not raw fetch: the interceptor carries
    // the workspace selection header and the active auth strategy (cookie CSRF
    // on web, bearer in the Tauri webview). The per-request Content-Type
    // override stops axios serialising the FormData as JSON; the browser then
    // replaces it with a boundary-carrying multipart header.
    const { data } = await apiClient.post<UploadedEditorImage[]>(
      `/documents/${docId}/images`,
      formData,
      { headers: { 'Content-Type': 'multipart/form-data' } }
    );

    const uploaded = data?.[0];
    if (!uploaded) {
      throw new EditorImageUploadError('upload-failed', 'No file returned from upload');
    }

    onProgress?.('Upload complete');
    logger.debug(`[EditorImage] Upload complete, URL: ${uploaded.url}`);

    return { url: uploaded.url, name: uploaded.name, size: uploaded.size };
  } catch (error) {
    if (error instanceof EditorImageUploadError) {
      throw error;
    }
    logger.error('Failed to upload editor image:', error);
    throw new EditorImageUploadError('upload-failed', 'Image upload failed', error);
  }
}

/**
 * Convert a dataURL to a File, or null when it is not base64 encoded.
 *
 * Returns null rather than throwing: the caller runs inside ProseMirror's
 * synchronous paste handler, and prosemirror-view calls
 * `event.preventDefault()` only AFTER that handler returns (see `doPaste` in
 * prosemirror-view/src/input.ts). An exception there would skip preventDefault
 * entirely, letting the browser paste the raw base64 img into the contentEditable
 * where the DOM observer reads it straight into the Yjs document.
 *
 * Percent-encoded data URLs are the common case that used to throw:
 * `data:image/svg+xml,<svg .../>` has no base64 payload, so `atob` rejects it.
 */
export function dataURLToFile(dataURL: string, filename: string): File | null {
  const [header, payload] = dataURL.split(',');
  if (!header?.includes(';base64') || !payload) return null;

  const mimeMatch = header.match(/:(.*?);/);
  const mime = mimeMatch ? mimeMatch[1] : 'image/png';

  let bstr: string;
  try {
    bstr = atob(payload);
  } catch {
    return null;
  }

  let n = bstr.length;
  const u8arr = new Uint8Array(n);
  while (n--) {
    u8arr[n] = bstr.charCodeAt(n);
  }

  return new File([u8arr], filename, { type: mime });
}

/**
 * Check if a string is a dataURL
 */
export function isDataURL(str: string): boolean {
  return str.startsWith('data:');
}

/**
 * Get file extension from MIME type
 */
export function getExtensionFromMime(mime: string): string {
  const mimeToExt: Record<string, string> = {
    'image/png': 'png',
    'image/jpeg': 'jpg',
    'image/gif': 'gif',
    'image/webp': 'webp',
    'image/svg+xml': 'svg',
    'image/bmp': 'bmp',
    'image/heic': 'heic',
    'image/heif': 'heif'
  };

  return mimeToExt[mime] || 'png';
}

/**
 * Generate a unique filename for an uploaded image
 */
export function generateImageFilename(mime: string): string {
  const ext = getExtensionFromMime(mime);
  const timestamp = Date.now();
  const random = Math.random().toString(36).substring(2, 8);
  return `editor-image-${timestamp}-${random}.${ext}`;
}

export default {
  uploadEditorImage,
  dataURLToFile,
  isDataURL,
  getExtensionFromMime,
  generateImageFilename
};
