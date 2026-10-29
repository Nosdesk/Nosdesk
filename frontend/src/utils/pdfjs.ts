/**
 * One place that loads pdf.js and opens documents, so every caller gets the
 * same worker and the same decoder assets.
 *
 * The assets (wasm image decoders, CMaps, standard fonts, ICC profiles) are
 * copied into `/pdfjs/<version>/` at build time by the `pdfjsAssets` plugin in
 * vite.config.ts. Both sides read the version of the installed pdfjs-dist, so
 * the worker and its assets always match, and the immutable cache on `/pdfjs/`
 * can't serve a previous release's decoders to a new worker.
 */
import type { PDFDocumentLoadingTask } from 'pdfjs-dist'

type Pdfjs = typeof import('pdfjs-dist')

let pdfjsPromise: Promise<Pdfjs> | null = null

/**
 * Load pdf.js once and point it at the minified worker, which Vite emits as
 * a hashed asset for the `?url` import. (Under `vite dev` the worker fails to
 * start and pdf.js runs on the main thread instead; builds, including the
 * dev stack's `vite build --watch`, use the real worker.)
 */
export function loadPdfjs(): Promise<Pdfjs> {
  pdfjsPromise ??= Promise.all([import('pdfjs-dist'), import('pdfjs-dist/build/pdf.worker.min.mjs?url')])
    .then(([pdfjs, worker]) => {
      pdfjs.GlobalWorkerOptions.workerSrc = worker.default
      return pdfjs
    })
    .catch((error: unknown) => {
      pdfjsPromise = null
      throw error
    })
  return pdfjsPromise
}

type PdfjsViewer = typeof import('pdfjs-dist/web/pdf_viewer.mjs')

let viewerPromise: Promise<PdfjsViewer> | null = null

/**
 * Load pdf.js's viewer components and their stylesheet (scoped to
 * `.pdfjs-scope` at build time). The components read `globalThis.pdfjsLib`,
 * which pdf.js sets when it loads, so pdf.js comes first.
 */
export function loadPdfjsViewer(): Promise<PdfjsViewer> {
  viewerPromise ??= loadPdfjs()
    .then(() => Promise.all([import('pdfjs-dist/web/pdf_viewer.mjs'), import('pdfjs-dist/web/pdf_viewer.css')]))
    .then(([viewer]) => viewer)
    .catch((error: unknown) => {
      viewerPromise = null
      throw error
    })
  return viewerPromise
}

const pdfjsAssetBase = (version: string) => new URL(`/pdfjs/${version}/`, document.baseURI).href

/** Where the viewer finds the icons a text annotation shows. */
export const pdfjsImagesUrl = (version: string) => `${pdfjsAssetBase(version)}images/`

/** Absolute URLs of the decoder assets for this pdf.js version. */
export function pdfjsAssetUrls(version: string) {
  const base = pdfjsAssetBase(version)
  return {
    wasmUrl: `${base}wasm/`,
    cMapUrl: `${base}cmaps/`,
    standardFontDataUrl: `${base}standard_fonts/`,
    iccUrl: `${base}iccs/`,
  }
}

export type PdfPasswordReason = 'needed' | 'incorrect'

/**
 * Asks the user for a document's password. Resolves to the password, or to
 * null when they cancel, which ends the load with a PdfPasswordCancelled.
 */
export type PdfPasswordRequest = (reason: PdfPasswordReason) => Promise<string | null>

/** Loads ended by a cancelled password prompt. */
const cancelledTasks = new WeakSet<PDFDocumentLoadingTask>()

interface OpenPdfOptions {
  onPassword?: PdfPasswordRequest
}

/**
 * Start loading a PDF from its bytes. Without `onPassword`, a protected
 * document fails with pdf.js's PasswordException, which suits thumbnails.
 */
export function openPdfDocument(
  pdfjs: Pdfjs,
  data: Uint8Array,
  options: OpenPdfOptions = {},
): PDFDocumentLoadingTask {
  const loadingTask = pdfjs.getDocument({
    data,
    cMapPacked: true,
    ...pdfjsAssetUrls(pdfjs.version),
  })

  const { onPassword } = options
  if (onPassword) {
    loadingTask.onPassword = (update: (password: string) => void, reason: number) => {
      const why = reason === pdfjs.PasswordResponses.INCORRECT_PASSWORD ? 'incorrect' : 'needed'
      void onPassword(why).then((password) => {
        if (password === null) {
          cancelledTasks.add(loadingTask)
          void loadingTask.destroy()
        } else {
          update(password)
        }
      })
    }
  }

  return loadingTask
}

/**
 * Wait for a document, turning a cancelled password prompt into a
 * PdfPasswordCancelled instead of pdf.js's generic abort.
 */
export async function awaitPdfDocument(loadingTask: PDFDocumentLoadingTask) {
  try {
    return await loadingTask.promise
  } catch (error) {
    if (cancelledTasks.has(loadingTask)) throw new PdfPasswordCancelled()
    throw error
  }
}

/** The user closed the password prompt. */
export class PdfPasswordCancelled extends Error {
  constructor() {
    super('PDF password prompt cancelled')
    this.name = 'PdfPasswordCancelled'
  }
}

/** The file couldn't be downloaded. `status` is set for an HTTP error. */
export class PdfFetchError extends Error {
  readonly status: number | null

  constructor(status: number | null, message: string) {
    super(message)
    this.name = 'PdfFetchError'
    this.status = status
  }
}

/** Bytes received so far, and the total when the server says. */
export interface PdfDownloadProgress {
  loaded: number
  total: number | null
}

/**
 * Download a PDF's bytes, reporting progress as they arrive. Throws
 * PdfFetchError on an HTTP or network error.
 */
export async function fetchPdfBytes(
  src: string,
  onProgress?: (progress: PdfDownloadProgress) => void,
): Promise<Uint8Array> {
  let response: Response
  try {
    // Default credentials: the web app's session cookie still goes with a
    // same-origin request, and the mobile app's asset proxy (which answers
    // with a wildcard CORS origin) accepts the request, which it refuses
    // when credentials are included.
    response = await fetch(src)
  } catch (error) {
    throw new PdfFetchError(null, error instanceof Error ? error.message : String(error))
  }
  if (!response.ok) {
    throw new PdfFetchError(response.status, `HTTP ${response.status}`)
  }
  if (!onProgress || !response.body) return new Uint8Array(await response.arrayBuffer())

  const length = Number(response.headers.get('Content-Length'))
  const total = Number.isFinite(length) && length > 0 ? length : null
  const chunks: Uint8Array[] = []
  let loaded = 0
  const reader = response.body.getReader()
  try {
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      chunks.push(value)
      loaded += value.byteLength
      onProgress({ loaded, total })
    }
  } catch (error) {
    throw new PdfFetchError(null, error instanceof Error ? error.message : String(error))
  }
  const bytes = new Uint8Array(loaded)
  let offset = 0
  for (const chunk of chunks) {
    bytes.set(chunk, offset)
    offset += chunk.byteLength
  }
  return bytes
}

export type PdfFailure = 'not-found' | 'invalid' | 'network' | 'password-cancelled' | 'unknown'

/**
 * What went wrong, for the message the viewer shows. pdf.js errors cross the
 * worker boundary as plain copies, so they're matched by name.
 */
export function classifyPdfError(error: unknown): PdfFailure {
  if (error instanceof PdfPasswordCancelled) return 'password-cancelled'
  if (error instanceof PdfFetchError) {
    if (error.status === null) return 'network'
    // The file API answers 404 for a file the user can't see, as well as for
    // one that's gone, so both read as "not available".
    if (error.status === 403 || error.status === 404 || error.status === 410) {
      return 'not-found'
    }
    return 'unknown'
  }
  const name = (error as { name?: unknown } | null)?.name
  if (name === 'InvalidPDFException') return 'invalid'
  return 'unknown'
}
