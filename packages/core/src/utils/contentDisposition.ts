/**
 * The file name a `Content-Disposition` header gives: the exact
 * `filename*=UTF-8''...` (RFC 6266) when present, else the plain `filename`,
 * quoted or not. The server sends the plain form, and adds `filename*` when
 * the plain one had to lose characters (an accent, a quote), so reading only
 * the plain one saves `R_sum_.pdf` for `Résumé.pdf`. Null without a name.
 *
 * The mobile app's native download reads the header the same way
 * (`mobile/src-tauri/src/share_file.rs`).
 */
export function fileNameFromDisposition(header: string | null | undefined): string | null {
  if (!header) return null
  const encoded = /filename\*\s*=\s*UTF-8''([^;]+)/i.exec(header)
  if (encoded) {
    try {
      return decodeURIComponent(encoded[1].trim())
    } catch {
      // Fall through to the plain parameter.
    }
  }
  const plain = /filename\s*=\s*"?([^";]+)"?/i.exec(header)
  return plain?.[1].trim() || null
}
