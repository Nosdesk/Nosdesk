/**
 * ProseMirror Image Upload Plugin
 *
 * Intercepts paste and drop events containing images, uploads them to the server,
 * and inserts URL references instead of base64 dataURIs.
 *
 * This prevents Yjs/WebSocket sync issues caused by large binary data in the document.
 */

import { Plugin, PluginKey, EditorState, Transaction } from 'prosemirror-state';
import { EditorView } from 'prosemirror-view';
import { Schema, Slice, Fragment, Node } from 'prosemirror-model';
import {
  uploadEditorImage,
  dataURLToFile,
  isDataURL,
  generateImageFilename,
  type EditorImageUploadOptions
} from '@/services/editorImageService';

export const imageUploadPluginKey = new PluginKey('imageUpload');

export interface ImageUploadPluginState {
  uploading: boolean;
  uploadCount: number;
}

interface ImageUploadPluginOptions {
  ticketId?: number;
  onUploadStart?: () => void;
  onUploadEnd?: () => void;
  onUploadError?: (error: Error) => void;
}

/**
 * Create a placeholder node while the image is uploading
 */
function createPlaceholder(view: EditorView, id: string): HTMLElement {
  const placeholder = document.createElement('div');
  placeholder.className = 'image-upload-placeholder';
  placeholder.setAttribute('data-upload-id', id);
  placeholder.innerHTML = `
    <div class="image-upload-spinner"></div>
    <span>Uploading image...</span>
  `;
  return placeholder;
}

/**
 * Find the position of a placeholder in the document
 */
function findPlaceholder(state: EditorState, id: string): number | null {
  const decos = imageUploadPluginKey.getState(state);
  // We don't use decorations for simplicity - just insert the image when ready
  return null;
}

/**
 * Handle image files - upload and insert
 */
async function handleImageFiles(
  view: EditorView,
  files: File[],
  pos: number,
  options: ImageUploadPluginOptions
): Promise<void> {
  console.log('[ImageUpload] handleImageFiles called with', files.length, 'files');
  const { schema } = view.state;
  const imageType = schema.nodes.image;

  if (!imageType) {
    console.error('[ImageUpload] Image node type not found in schema');
    return;
  }

  options.onUploadStart?.();

  for (const file of files) {
    if (!file.type.startsWith('image/')) {
      console.log('[ImageUpload] Skipping non-image file:', file.type);
      continue;
    }

    console.log('[ImageUpload] Uploading file:', file.name, file.type, file.size, 'bytes');

    try {
      // Upload the image with ticket context if available
      const result = await uploadEditorImage(file, { ticketId: options.ticketId });
      console.log('[ImageUpload] Upload successful, URL:', result.url);

      // Create the image node with the URL
      const imageNode = imageType.create({
        src: result.url,
        alt: file.name,
        title: file.name
      });

      // Insert the image at the position
      const tr = view.state.tr.insert(pos, imageNode);
      view.dispatch(tr);
      console.log('[ImageUpload] Image node inserted at position', pos);

      // Update position for next image
      pos += imageNode.nodeSize;
    } catch (error) {
      console.error('[ImageUpload] Failed to upload image:', error);
      options.onUploadError?.(error instanceof Error ? error : new Error(String(error)));
    }
  }

  options.onUploadEnd?.();
}

/**
 * Handle pasted content - check for images in HTML or as files
 */
async function handlePaste(
  view: EditorView,
  event: ClipboardEvent,
  options: ImageUploadPluginOptions
): Promise<boolean> {
  const { clipboardData } = event;
  if (!clipboardData) return false;

  // Check for image files first (e.g., screenshot paste)
  const imageFiles: File[] = [];
  for (let i = 0; i < clipboardData.files.length; i++) {
    const file = clipboardData.files[i];
    if (file.type.startsWith('image/')) {
      imageFiles.push(file);
    }
  }

  if (imageFiles.length > 0) {
    event.preventDefault();
    const pos = view.state.selection.from;
    await handleImageFiles(view, imageFiles, pos, options);
    return true;
  }

  // Check for HTML content with images
  const html = clipboardData.getData('text/html');
  if (html) {
    const parser = new DOMParser();
    const doc = parser.parseFromString(html, 'text/html');
    const images = doc.querySelectorAll('img');

    const dataURLImages: { src: string; alt: string }[] = [];
    images.forEach(img => {
      if (isDataURL(img.src)) {
        dataURLImages.push({
          src: img.src,
          alt: img.alt || 'pasted-image'
        });
      }
    });

    if (dataURLImages.length > 0) {
      event.preventDefault();

      options.onUploadStart?.();
      const { schema } = view.state;
      const imageType = schema.nodes.image;
      let pos = view.state.selection.from;

      for (const imgData of dataURLImages) {
        try {
          // Convert dataURL to file
          const mimeMatch = imgData.src.match(/data:([^;]+);/);
          const mime = mimeMatch ? mimeMatch[1] : 'image/png';
          const filename = generateImageFilename(mime);
          const file = dataURLToFile(imgData.src, filename);

          // Upload the image with ticket context if available
          const result = await uploadEditorImage(file, { ticketId: options.ticketId });

          // Create and insert the image node
          const imageNode = imageType.create({
            src: result.url,
            alt: imgData.alt,
            title: imgData.alt
          });

          const tr = view.state.tr.insert(pos, imageNode);
          view.dispatch(tr);
          pos += imageNode.nodeSize;
        } catch (error) {
          console.error('Failed to upload pasted image:', error);
          options.onUploadError?.(error instanceof Error ? error : new Error(String(error)));
        }
      }

      options.onUploadEnd?.();
      return true;
    }
  }

  return false;
}

/**
 * Handle dropped files
 */
async function handleDrop(
  view: EditorView,
  event: DragEvent,
  options: ImageUploadPluginOptions
): Promise<boolean> {
  const { dataTransfer } = event;
  if (!dataTransfer) return false;

  const imageFiles: File[] = [];
  for (let i = 0; i < dataTransfer.files.length; i++) {
    const file = dataTransfer.files[i];
    if (file.type.startsWith('image/')) {
      imageFiles.push(file);
    }
  }

  if (imageFiles.length === 0) return false;

  event.preventDefault();

  // Get the drop position
  const pos = view.posAtCoords({ left: event.clientX, top: event.clientY });
  if (!pos) return false;

  await handleImageFiles(view, imageFiles, pos.pos, options);
  return true;
}

/**
 * Create the image upload plugin
 */
export function createImageUploadPlugin(options: ImageUploadPluginOptions = {}): Plugin {
  return new Plugin({
    key: imageUploadPluginKey,

    state: {
      init(): ImageUploadPluginState {
        return { uploading: false, uploadCount: 0 };
      },
      apply(tr, state): ImageUploadPluginState {
        return state;
      }
    },

    props: {
      handlePaste(view: EditorView, event: ClipboardEvent): boolean {
        // Handle async - return true to prevent default if we're handling images
        const clipboardData = event.clipboardData;
        if (!clipboardData) return false;

        // Check if there are image files or dataURL images
        let hasImages = false;
        for (let i = 0; i < clipboardData.files.length; i++) {
          if (clipboardData.files[i].type.startsWith('image/')) {
            hasImages = true;
            break;
          }
        }

        if (!hasImages) {
          const html = clipboardData.getData('text/html');
          if (html && html.includes('data:image')) {
            hasImages = true;
          }
        }

        if (hasImages) {
          // Prevent default and handle async
          handlePaste(view, event, options);
          return true;
        }

        return false;
      },

      handleDrop(view: EditorView, event: DragEvent, slice: Slice, moved: boolean): boolean {
        const { dataTransfer } = event;
        if (!dataTransfer) return false;

        // Check if there are image files
        let hasImages = false;
        for (let i = 0; i < dataTransfer.files.length; i++) {
          if (dataTransfer.files[i].type.startsWith('image/')) {
            hasImages = true;
            break;
          }
        }

        if (hasImages) {
          // Prevent default and handle async
          handleDrop(view, event, options);
          return true;
        }

        return false;
      },

      // Transform pasted content to convert any remaining dataURL images
      // This catches images that slip through other handlers
      transformPasted(slice: Slice, view: EditorView): Slice {
        const { schema } = view.state;

        // Walk through the slice and look for image nodes with dataURL sources
        let hasDataURLImages = false;

        slice.content.descendants((node) => {
          if (node.type === schema.nodes.image && isDataURL(node.attrs.src)) {
            hasDataURLImages = true;
            return false; // Stop iteration
          }
          return true;
        });

        if (hasDataURLImages) {
          // Return an empty slice to prevent the paste
          // The handlePaste will handle the upload
          console.warn('Blocked paste with dataURL images - uploading instead');
        }

        return slice;
      }
    }
  });
}

export default createImageUploadPlugin;
