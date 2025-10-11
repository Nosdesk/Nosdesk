import heic2any from 'heic2any';

/**
 * Upload Service
 * Provides utilities for file processing, validation, and format conversion
 */

interface FileValidationOptions {
  maxSizeMB?: number;
  allowedTypes?: string[];
}

class UploadService {
  /**
   * Convert HEIC/HEIF images to WebP format
   */
  async convertHeicToJpeg(file: File, onProgress?: (message: string) => void): Promise<File> {
    const isHeic = file.type === 'image/heic' ||
                   file.type === 'image/heif' ||
                   file.name.toLowerCase().endsWith('.heic') ||
                   file.name.toLowerCase().endsWith('.heif');

    if (!isHeic) {
      return file;
    }

    try {
      const message = 'Converting HEIC image to WebP...';
      console.log(message);
      if (onProgress) onProgress(message);

      const convertedBlob = await heic2any({
        blob: file,
        toType: 'image/webp',
        quality: 0.9
      });

      // heic2any can return an array of Blobs for multi-frame HEIC
      const blob = Array.isArray(convertedBlob) ? convertedBlob[0] : convertedBlob;

      const newFile = new File(
        [blob],
        file.name.replace(/\.heic$/i, '.webp').replace(/\.heif$/i, '.webp'),
        { type: 'image/webp' }
      );

      const successMessage = 'Image converted successfully';
      console.log(successMessage);
      if (onProgress) onProgress(successMessage);

      return newFile;
    } catch (error) {
      console.error('Error converting HEIC:', error);
      throw new Error('Failed to convert HEIC image. Please try a different image format.');
    }
  }

  /**
   * Validate file against size and type constraints
   */
  validateFile(file: File, options: FileValidationOptions = {}): { valid: boolean; error?: string } {
    const { maxSizeMB = 50, allowedTypes } = options;

    // Check file type if specified
    if (allowedTypes && allowedTypes.length > 0) {
      const isAllowed = allowedTypes.some(type => {
        if (type.endsWith('/*')) {
          return file.type.startsWith(type.replace('/*', '/'));
        }
        return file.type === type;
      });

      if (!isAllowed) {
        return { valid: false, error: `File type not allowed. Allowed types: ${allowedTypes.join(', ')}` };
      }
    }

    // Check file size
    const maxSize = maxSizeMB * 1024 * 1024;
    if (file.size > maxSize) {
      return { valid: false, error: `File must be less than ${maxSizeMB}MB` };
    }

    return { valid: true };
  }

  /**
   * Create object URL for file preview
   */
  createPreviewUrl(file: File): string {
    return URL.createObjectURL(file);
  }

  /**
   * Revoke object URL to free memory
   */
  revokePreviewUrl(url: string): void {
    URL.revokeObjectURL(url);
  }
}

export default new UploadService();
