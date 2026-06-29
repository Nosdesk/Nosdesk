import { computed, onScopeDispose, ref, watch, type Ref } from 'vue'

/**
 * Resolve an `<img>` src that may have a local blob preview, with no flash.
 *
 * When `preview` (a local object URL we already hold) is given, it is shown
 * instantly and we swap to `target` (the server image) only once it has
 * decoded, then revoke the blob, so the local -> server handoff is seamless.
 * Without a preview it simply tracks `target`.
 *
 * Used by `AttachmentPreview` so a just-sent image stays visible across the
 * optimistic -> reconciled remount (the blob comes from `attachmentPreviewCache`).
 */
export function useSmoothImageSrc(target: () => string, preview?: string | null): Ref<string> {
  const targetUrl = computed(target)
  const displayed = ref(preview || targetUrl.value)
  let blob: string | null = preview ?? null

  const revokeBlob = () => {
    if (blob) {
      URL.revokeObjectURL(blob)
      blob = null
    }
  }

  watch(
    targetUrl,
    (url) => {
      // No blob to bridge: just follow the target.
      if (!url || !blob) {
        displayed.value = url
        return
      }
      // Hold the blob; reveal the server image only after it has decoded.
      const img = new Image()
      const swap = () => {
        displayed.value = url
        revokeBlob()
      }
      if (img.decode) {
        img.src = url
        // On decode failure keep showing the local preview (revoked on dispose).
        void img.decode().then(swap).catch(() => {})
      } else {
        img.onload = swap
        img.src = url
      }
    },
    { immediate: true },
  )

  onScopeDispose(revokeBlob)
  return displayed
}
