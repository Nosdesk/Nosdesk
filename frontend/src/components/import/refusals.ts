/**
 * Why the server refused a file or a sheet, in the viewer's language: the
 * refusal's `code` picks the wording and its `args` fill it in. A code
 * with no wording here shows the server's English.
 */

import { formatFileSize } from '@nosdesk/core/utils/formatFileSize'

type Translate = (key: string, args?: Record<string, string | number>) => string

const REFUSAL_KEYS: Record<string, string> = {
  import_file_empty: 'import-refused-file-empty',
  import_file_too_large: 'import-refused-file-too-large',
  import_xlsx_protected: 'import-refused-xlsx-protected',
  import_xlsx_unreadable: 'import-refused-xlsx-unreadable',
  import_xlsx_too_large: 'import-refused-xlsx-too-large',
  import_xlsx_too_many_parts: 'import-refused-xlsx-too-many-parts',
  import_xlsx_no_rows: 'import-refused-xlsx-no-rows',
  import_sheets_hidden: 'import-refused-sheets-hidden',
  import_sheet_unknown: 'import-refused-sheet-unknown',
  import_sheet_hidden: 'import-refused-sheet-hidden',
  import_sheet_already_imported: 'import-refused-sheet-already-imported',
  import_sheet_empty: 'import-refused-sheet-empty',
  import_header_row_empty: 'import-refused-header-row-empty',
  import_sheet_too_many_rows: 'import-refused-sheet-too-many-rows',
  import_sheet_no_rows: 'import-refused-sheet-no-rows',
  import_sheet_too_wide: 'import-refused-sheet-too-wide',
  import_header_too_long: 'import-refused-header-too-long',
  import_cell_too_long: 'import-refused-cell-too-long',
  import_csv_unreadable: 'import-refused-csv-unreadable',
  import_csv_not_utf8: 'import-refused-csv-not-utf8',
  import_no_header: 'import-refused-no-header',
  import_header_duplicate: 'import-refused-header-duplicate',
  import_header_blank: 'import-refused-header-blank',
  import_too_many_columns: 'import-refused-too-many-columns',
  import_too_many_rows: 'import-refused-too-many-rows',
  import_no_rows: 'import-refused-no-rows',
  import_parse_timed_out: 'import-refused-timed-out',
  // Documentation imports.
  archive_unreadable: 'docs-import-refused-not-a-zip',
  archive_too_many_entries: 'docs-import-refused-too-many-files',
  archive_encrypted: 'docs-import-refused-encrypted',
  archive_unsafe_path: 'docs-import-refused-unsafe-path',
  archive_duplicate_entry: 'docs-import-refused-duplicate-entry',
  archive_compression_ratio: 'docs-import-refused-compression-ratio',
  archive_entry_too_large: 'docs-import-refused-entry-too-large',
  archive_too_large: 'docs-import-refused-unpacked-too-large',
  archive_entry_missing: 'docs-import-refused-entry-damaged',
  archive_entry_corrupt: 'docs-import-refused-entry-damaged',
  archive_io: 'docs-import-refused-unreadable-upload',
  upload_unreadable: 'docs-import-refused-unreadable-upload',
  upload_not_saved: 'docs-import-refused-unreadable-upload',
  import_no_file: 'docs-import-refused-no-file',
  upload_too_large: 'docs-import-refused-too-large',
  docs_import_mixed_files: 'docs-import-refused-mixed-files',
  docs_import_unreadable: 'docs-import-refused-unreadable',
  docs_import_timed_out: 'docs-import-refused-timed-out',
  docs_import_no_markdown: 'docs-import-refused-no-markdown',
  docs_import_too_many_pages: 'docs-import-refused-too-many-pages',
  docs_import_source_later: 'docs-import-refused-source-later',
  docs_import_collection_missing: 'docs-import-refused-collection-missing',
  import_job_busy: 'docs-import-refused-busy',
  import_check_running: 'docs-import-refused-check-running',
  import_commit_running: 'docs-import-refused-commit-running',
  import_type_busy: 'docs-import-refused-type-busy',
  import_committed: 'docs-import-refused-committed',
  import_not_checked: 'docs-import-refused-not-checked',
  import_expired: 'docs-import-refused-expired',
  import_file_missing: 'import-file-missing',
}

/** Refusals whose `max` is a size in bytes, shown as one. */
const SIZE_ARGS = new Set(['upload_too_large', 'archive_entry_too_large', 'archive_too_large'])

export interface ErrorBody {
  error?: string
  code?: string
  args?: Record<string, string | number>
}

/** The error body of a failed request, if it has one. */
export function errorBody(e: unknown): ErrorBody | undefined {
  return (e as { response?: { data?: ErrorBody } })?.response?.data
}

/** A refusal's wording by its code, else the server's English; `null`
 *  when the body says neither. `extra` fills arguments the server doesn't
 *  send (the file's name and size). */
export function refusalText(
  t: Translate,
  body: ErrorBody | undefined,
  extra: Record<string, string | number> = {},
): string | null {
  if (!body) return null
  const key = body.code ? REFUSAL_KEYS[body.code] : undefined
  if (key) {
    const args: Record<string, string | number> = { ...extra, ...(body.args ?? {}) }
    if (body.code && SIZE_ARGS.has(body.code) && typeof args.max === 'number') {
      args.max = formatFileSize(args.max)
    }
    // The zip inside a split export has its own wording.
    if (body.code === 'archive_unreadable' && args.part) return t('docs-import-refused-inner-zip', args)
    return t(key, args)
  }
  return typeof body.error === 'string' ? body.error : null
}

/** The wording of each reason a removal found nothing to delete. */
const NOTHING_TO_REMOVE_KEYS: Record<string, string> = {
  none_created: 'import-remove-nothing-none-created',
  all_gone: 'import-remove-nothing-all-gone',
  all_changed: 'import-remove-nothing-all-changed',
}

/** Why Remove imported assets was refused, for the refusals that leave
 *  the job as it was: nothing to delete (by `args.reason`) or an import
 *  that wasn't committed. `null` for any other refusal. */
export function removalRefusalText(t: Translate, body: ErrorBody | undefined): string | null {
  switch (body?.code) {
    case 'import_nothing_to_remove': {
      const reason = body.args?.reason
      const key = typeof reason === 'string' ? NOTHING_TO_REMOVE_KEYS[reason] : undefined
      if (key) return t(key)
      return typeof body.error === 'string' ? body.error : null
    }
    case 'import_not_committed':
      return t('import-remove-not-committed')
    default:
      return null
  }
}

/** Why Remove imported pages or Publish all was refused, worded for pages.
 *  `closes` fills the closed window's date. `null` for a failure that isn't
 *  a refusal. */
export function docsActionRefusalText(t: Translate, body: ErrorBody | undefined, closes = ''): string | null {
  switch (body?.code) {
    case 'import_not_committed':
      return t('docs-import-remove-not-committed')
    case 'import_already_removed':
      return t('docs-import-remove-already-removed')
    case 'import_removal_window_closed':
      return t('import-remove-window-closed', { date: closes })
    case 'import_busy':
      return t('docs-import-action-busy')
    case 'import_type_busy':
      return t('docs-import-refused-type-busy')
    case 'import_nothing_to_remove': {
      const reason = body.args?.reason
      if (reason === 'none_created' || reason === 'all_gone' || reason === 'all_changed') {
        return t(`docs-import-remove-nothing-${reason.replace(/_/g, '-')}`)
      }
      return typeof body.error === 'string' ? body.error : null
    }
    default:
      return null
  }
}
