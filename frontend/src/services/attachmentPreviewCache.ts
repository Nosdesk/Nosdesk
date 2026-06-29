/**
 * One-shot bridge for the optimistic -> reconciled attachment swap.
 *
 * A just-sent file first renders as an optimistic pool row carrying a local
 * `blob:` URL (instant preview). When the comment reconciles, that row is
 * replaced by the authoritative row keyed by the file's server URL (a remount),
 * whose `<img>` would otherwise reload from the network and flash.
 *
 * `addComment` stashes the local blob here keyed by the final server URL; the
 * reconciled `AttachmentPreview` takes it (one-shot) and shows it until the
 * server image has decoded, then revokes it (see `useSmoothImageSrc`).
 */
const previews = new Map<string, string>()

/** Stash a file's local blob URL against the server URL it will reconcile to. */
export function stashPreview(serverUrl: string, blobUrl: string): void {
  previews.set(serverUrl, blobUrl)
}

/** Consume and return the stashed blob URL for a server URL, if any. */
export function takePreview(serverUrl: string): string | null {
  const blob = previews.get(serverUrl)
  if (blob) previews.delete(serverUrl)
  return blob ?? null
}
