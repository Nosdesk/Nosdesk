/**
 * Editor Image Upload Service
 *
 * Handles image uploads for the collaborative editor, converting
 * pasted/dropped images to URL references instead of storing
 * base64 dataURIs in the Yjs document.
 *
 * This prevents Yjs/WebSocket sync issues with large binary data.
 */

import uploadService from './uploadService';
import { logger } from '@/utils/logger';

export interface EditorImageUploadResult {
  url: string;
  name: string;
  id?: number;
  size?: number;
}

export interface EditorImageUploadOptions {
  ticketId?: number;
  onProgress?: (message: string) => void;
}

/**
 * Upload an image file for use in the collaborative editor
 * Returns a URL that can be used as the image src
 *
 * @param file - The image file to upload
 * @param options - Upload options including ticketId for ticket-specific storage
 */
export async function uploadEditorImage(
  file: File,
  options: EditorImageUploadOptions = {}
): Promise<EditorImageUploadResult> {
  const { ticketId, onProgress } = options;

  // Validate the file
  const validation = uploadService.validateFile(file, {
    maxSizeMB: 10, // 10MB limit for editor images
    allowedTypes: ['image/*']
  });

  if (!validation.valid) {
    throw new Error(validation.error || 'Invalid file');
  }

  // Convert HEIC to WebP if needed
  const processedFile = await uploadService.convertHeicToJpeg(file, onProgress);

  // Create form data for upload
  const formData = new FormData();
  // Explicitly pass filename as third argument
  formData.append('files', processedFile, processedFile.name);

  if (onProgress) {
    onProgress('Uploading image...');
  }

  // Get CSRF token from cookie for the request
  const csrfMatch = document.cookie.match(/csrf_token=([^;]+)/);
  const csrfToken = csrfMatch ? csrfMatch[1] : '';

  // Use ticket-specific endpoint if ticketId is provided
  const uploadUrl = ticketId
    ? `/api/tickets/${ticketId}/notes/images`
    : '/api/upload';

  logger.debug(`[EditorImage] Uploading to ${uploadUrl}`);

  try {
    // Use native fetch for FormData uploads - axios has issues with multipart boundary
    // when Content-Type is set in default headers
    const response = await fetch(uploadUrl, {
      method: 'POST',
      body: formData,
      credentials: 'same-origin',
      headers: {
        'X-CSRF-Token': csrfToken
      }
      // Don't set Content-Type - browser will set it correctly with boundary for FormData
    });

    if (!response.ok) {
      const errorText = await response.text();
      throw new Error(`Upload failed: ${response.status} ${errorText}`);
    }

    const data = await response.json();

    if (data && data.length > 0) {
      const uploadedFile = data[0];

      if (onProgress) {
        onProgress('Upload complete');
      }

      // The URL returned from the ticket-specific endpoint is in format:
      // /uploads/tickets/{ticketId}/notes/{uuid}_{filename}
      // We need to convert it to the API file serving endpoint:
      // /api/files/tickets/{ticketId}/notes/{uuid}_{filename}
      let finalUrl = uploadedFile.url;
      if (ticketId && finalUrl.startsWith('/uploads/')) {
        finalUrl = '/api/files/' + finalUrl.substring('/uploads/'.length);
      }

      logger.debug(`[EditorImage] Upload complete, URL: ${finalUrl}`);

      return {
        url: finalUrl,
        name: uploadedFile.name,
        id: uploadedFile.id,
        size: uploadedFile.size
      };
    }

    throw new Error('No file returned from upload');
  } catch (error) {
    logger.error('Failed to upload editor image:', error);
    throw error;
  }
}

/**
 * Convert a dataURL to a File object
 */
export function dataURLToFile(dataURL: string, filename: string): File {
  const arr = dataURL.split(',');
  const mimeMatch = arr[0].match(/:(.*?);/);
  const mime = mimeMatch ? mimeMatch[1] : 'image/png';
  const bstr = atob(arr[1]);
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
