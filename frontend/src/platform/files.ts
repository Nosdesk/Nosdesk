/**
 * Saving a file to the user's device, on whichever host the app runs in.
 *
 * Every download goes through `saveFile`, or through `<DownloadLink>`, which
 * calls it; the `nosdesk/file-download` lint keeps raw `download` links out of
 * the rest of the app. The hosts differ:
 *
 * - Web: the browser saves it. A server file is an `<a href download>` the
 *   browser follows with the session cookie, streaming to disk with its own
 *   progress, so a large backup never sits in page memory. Bytes the page
 *   holds go through an object URL.
 * - App: the webview can't save anything (`<a download>` does nothing and a
 *   new window is refused), so the file goes to the native share sheet, where
 *   Save to Files is one of the actions. A server file is downloaded by Rust
 *   with the session (`stage_server_file`) and never crosses the webview;
 *   bytes the page holds go over IPC.
 *
 * A caller that needs the server's error response (an import report, the
 * documentation export) fetches through the API client and saves the bytes,
 * which in the app cross the webview twice. Large files with nothing to say
 * on failure (attachments, backups, workspace exports) are saved by path.
 *
 * In the app the share folder holds one file at a time, so saves run one
 * after another. Android's chooser reports done as soon as it opens, so there
 * a quick second save can replace a file the chosen app has yet to read. The
 * `@nosdesk/mobile` import stays dynamic so the web bundle never pulls the
 * native chunk.
 */
import { assetPath, isServerFilePath } from '@nosdesk/core/transport'
import { convertToAuthenticatedPath } from '@/services/fileService'
import { isTauriRuntime } from './runtime'

/**
 * What to save: a stored server path (`/api/...`, `/uploads/...`, or one
 * already resolved for display), a `blob:` or `data:` URL, or bytes.
 */
export type FileSource = string | Blob

export interface SaveResult {
  /**
   * False when the user dismissed the app's share sheet. On the web the
   * browser takes the file and this is always true.
   */
  completed: boolean
}

/**
 * Save `source` as `filename`. Without a name, a server file is named as the
 * server names it. Rejects when the file can't be fetched or the share sheet
 * can't open.
 */
export function saveFile(source: FileSource, filename?: string): Promise<SaveResult> {
  if (!isTauriRuntime()) return Promise.resolve(saveInBrowser(source, filename))
  return oneAtATime(() => saveInApp(source, filename))
}

/**
 * Hand bytes to the OS share sheet: Save to Files, AirDrop, Mail and Print on
 * iOS, the app chooser on Android. Native only: outside the Tauri shell this
 * throws, and the caller keeps its web path. Resolves with `completed: false`
 * when the user dismisses the sheet.
 */
export function shareFile(
  bytes: Uint8Array,
  filename: string,
  mimeType: string,
): Promise<SaveResult> {
  if (!isTauriRuntime()) return Promise.reject(new Error('shareFile needs the native app'))
  return oneAtATime(async () => {
    const { shareFile: nativeShareFile } = await import('@nosdesk/mobile')
    return nativeShareFile(bytes, filename, mimeType)
  })
}

/** The URL a link to `source` points at on this host. */
export function fileHref(source: string): string {
  return isBlobUrl(source) ? source : convertToAuthenticatedPath(source)
}

function saveInBrowser(source: FileSource, filename?: string): SaveResult {
  if (typeof source === 'string') {
    clickDownload(fileHref(source), filename)
    return { completed: true }
  }
  const url = URL.createObjectURL(source)
  clickDownload(url, filename)
  // The browser reads the object URL after the click returns; revoking at
  // once can cancel the save in some engines.
  setTimeout(() => URL.revokeObjectURL(url), 60_000)
  return { completed: true }
}

function clickDownload(href: string, filename?: string): void {
  const link = document.createElement('a')
  link.href = href
  // An empty name lets the server's Content-Disposition name the file.
  link.download = filename ?? ''
  link.style.display = 'none'
  document.body.appendChild(link)
  link.click()
  link.remove()
}

async function saveInApp(source: FileSource, filename?: string): Promise<SaveResult> {
  const mobile = await import('@nosdesk/mobile')
  if (typeof source === 'string' && !isBlobUrl(source)) {
    // Back from the proxy scheme to the stored path Rust fetches.
    const path = assetPath(convertToAuthenticatedPath(source))
    if (!isServerFilePath(path)) throw new Error(`not a server file: ${source}`)
    return mobile.shareServerFile(path, filename)
  }
  const blob = typeof source === 'string' ? await (await fetch(source)).blob() : source
  const bytes = new Uint8Array(await blob.arrayBuffer())
  return mobile.shareFile(bytes, filename ?? 'file', blob.type || 'application/octet-stream')
}

function isBlobUrl(source: string): boolean {
  return source.startsWith('blob:') || source.startsWith('data:')
}

let queue: Promise<unknown> = Promise.resolve()

function oneAtATime<T>(task: () => Promise<T>): Promise<T> {
  const run = queue.then(task, task)
  queue = run.catch(() => {})
  return run
}
