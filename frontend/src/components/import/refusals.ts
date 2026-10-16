/**
 * Why the server refused a file or a sheet, in the viewer's language: the
 * refusal's `code` picks the wording and its `args` fill it in. A code
 * with no wording here shows the server's English.
 */

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
}

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
 *  when the body says neither. */
export function refusalText(t: Translate, body: ErrorBody | undefined): string | null {
  if (!body) return null
  const key = body.code ? REFUSAL_KEYS[body.code] : undefined
  if (key) return t(key, body.args ?? {})
  return typeof body.error === 'string' ? body.error : null
}
