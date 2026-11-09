/**
 * One reply or note attachment uploaded to `/upload` on its own request, so
 * each file has its own progress, cancel and retry. The server keeps it
 * unattached until a reply claims it by id, and deletes it after a day if
 * none does.
 */
import apiClient from '../apiClient'
import { isCancellation } from '../utils/cancellation'
import { UPLOAD_TOO_LARGE } from './uploadLimits'

/** An upload the server stored, as `/upload` answers it. */
export interface UploadedAttachment {
  id: number
  url: string
  name: string
  transcription?: string | null
  thumbnail_url?: string | null
}

/** Why an upload failed, in terms a message can be chosen from. */
export type UploadFailure =
  | 'too-large'
  | 'type-not-allowed'
  | 'invalid-name'
  | 'busy'
  | 'offline'
  | 'failed'

export interface UploadOptions {
  /** A voice note's transcription. The server applies it to the file after it. */
  transcription?: string
  signal?: AbortSignal
  /** Bytes sent so far, and the request's size when the transport knows it. */
  onProgress?: (loaded: number, total: number | null) => void
}

/** Upload one file. Rejects with the request's error. */
export async function uploadAttachment(
  file: File,
  options: UploadOptions = {},
): Promise<UploadedAttachment> {
  const form = new FormData()
  if (options.transcription) form.append('transcription', options.transcription)
  form.append('files', file, file.name)
  const { data } = await apiClient.post<UploadedAttachment[]>('/upload', form, {
    headers: { 'Content-Type': 'multipart/form-data' },
    // A large file takes as long as it takes; cancel is the way out.
    timeout: 0,
    signal: options.signal,
    onUploadProgress: options.onProgress
      ? (e) => options.onProgress?.(e.loaded, e.total ?? null)
      : undefined,
  })
  const stored = Array.isArray(data) ? data[0] : undefined
  if (!stored || typeof stored.id !== 'number') {
    throw new Error('The upload answered without the stored file')
  }
  return stored
}

/**
 * Why an upload failed, from the server's code and status, never its message.
 * Null for a cancelled upload, which isn't a failure.
 */
export function uploadFailure(err: unknown): UploadFailure | null {
  if (isCancellation(err)) return null
  const e = err as {
    code?: unknown
    response?: { status?: unknown; data?: { code?: unknown } }
  } | null
  const status = e?.response?.status
  const code = e?.response?.data?.code
  if (code === 'FILE_TYPE_NOT_ALLOWED') return 'type-not-allowed'
  if (code === 'INVALID_FILENAME') return 'invalid-name'
  if (code === 'FILE_TOO_LARGE' || status === 413 || e?.code === UPLOAD_TOO_LARGE) {
    return 'too-large'
  }
  if (status === 503) return 'busy'
  const unanswered =
    e?.code === 'ERR_NETWORK' || e?.code === 'ECONNABORTED' || e?.code === 'ETIMEDOUT'
  if (unanswered && !e?.response) return 'offline'
  return 'failed'
}
