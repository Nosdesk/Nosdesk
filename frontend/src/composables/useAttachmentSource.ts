import { computed, onScopeDispose, type ComputedRef } from 'vue'
import { convertToAuthenticatedPath } from '@/services/fileService'
import { takePreview } from '@/services/attachmentPreviewCache'

/**
 * The URL to render an attachment from, for ANY type (image / audio / video /
 * file), with one rule: the just-sent local blob if we have one, else the
 * authenticated server URL.
 *
 * A just-sent attachment hands off its local blob via the preview cache; we
 * render that directly so it appears instantly and survives the optimistic ->
 * reconciled swap with no network round-trip or flash. The blob is byte-
 * identical to the server copy, so there is nothing to decode-gate, we simply
 * keep rendering it. On a reloaded attachment there is no blob, so it resolves
 * to the server URL. Owns the blob's lifetime (revoked when the renderer
 * unmounts). Renderers pick the *element* by type but never branch on type for
 * *sourcing*.
 */
export function useAttachmentSource(url: () => string): ComputedRef<string> {
  const blob = takePreview(url())
  onScopeDispose(() => {
    if (blob) URL.revokeObjectURL(blob)
  })
  return computed(() => blob ?? convertToAuthenticatedPath(url()))
}
