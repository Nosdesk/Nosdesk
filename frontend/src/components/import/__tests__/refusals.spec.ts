import { describe, expect, it } from 'vitest'
import { FluentBundle, FluentResource } from '@fluent/bundle'

import enUS from '../../../../../i18n/locales/en-US/main.ftl?raw'
import { errorBody, refusalText } from '../refusals'
import { choiceLabel, localisesChoices } from '../choiceLabels'

const t = (key: string, args?: Record<string, string | number>) =>
  args && Object.keys(args).length > 0 ? `${key} ${JSON.stringify(args)}` : key

describe('refusals', () => {
  it('words a known code with its args', () => {
    expect(
      refusalText(t, { error: 'English', code: 'import_header_row_empty', args: { row: '2', sheet: 'Cameras' } }),
    ).toBe('import-refused-header-row-empty {"row":"2","sheet":"Cameras"}')
    expect(refusalText(t, { error: 'English', code: 'import_sheet_already_imported' })).toBe(
      'import-refused-sheet-already-imported',
    )
  })

  it('keeps the server English for an unknown code or none', () => {
    expect(refusalText(t, { error: 'English', code: 'BAD_REQUEST' })).toBe('English')
    expect(refusalText(t, { error: 'English' })).toBe('English')
    expect(refusalText(t, {})).toBeNull()
    expect(refusalText(t, undefined)).toBeNull()
  })

  it('reads the body of a failed request', () => {
    expect(errorBody({ response: { data: { code: 'x' } } })).toEqual({ code: 'x' })
    expect(errorBody(new Error('network'))).toBeUndefined()
  })
})

describe('choice labels', () => {
  it('words statuses, roles and priorities by value', () => {
    expect(localisesChoices('users', 'role')).toBe(true)
    expect(localisesChoices('tickets', 'priority')).toBe(true)
    expect(localisesChoices('assets', 'role')).toBe(false)
    expect(choiceLabel(t, 'users', 'role', 'technician', 'Technician')).toBe('user-mgmt-role-technician')
    expect(choiceLabel(t, 'users', 'role', 'audit_reviewer', 'Audit reviewer')).toBe('user-mgmt-role-audit_reviewer')
    expect(choiceLabel(t, 'tickets', 'priority', 'urgent', 'Urgent')).toBe('priority-urgent')
    expect(choiceLabel(t, 'assets', 'status', 'in_stock', 'In stock')).toBe('asset-status-in-stock')
  })

  it('keeps the server label for anything else', () => {
    expect(choiceLabel(t, 'users', 'role', 'owner', 'Owner')).toBe('Owner')
    expect(choiceLabel(t, 'tickets', 'workflow_state', 'Open', 'Open')).toBe('Open')
    expect(choiceLabel(t, 'assets', 'kind', 'camera', 'Camera')).toBe('Camera')
  })
})

describe('the en-US wording', () => {
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  bundle.addResource(new FluentResource(enUS))
  const real = (key: string, args?: Record<string, string | number>) => {
    const message = bundle.getMessage(key)
    if (!message?.value) throw new Error(`no message ${key}`)
    return bundle.formatPattern(message.value, args)
  }

  it('words refusals with their args', () => {
    expect(refusalText(real, { code: 'import_too_many_rows', args: { max: 5000 } })).toBe(
      'The file has more than 5,000 data rows. Split it and import it in smaller runs.',
    )
    expect(refusalText(real, { code: 'import_header_row_empty', args: { row: '2', sheet: 'Cameras' } })).toBe(
      "Row 2 of 'Cameras' is empty. Choose the row that holds the column headers.",
    )
    expect(refusalText(real, { code: 'import_sheets_hidden', args: {} })).toBe(
      'The rows in this workbook are on hidden sheets. Unhide the sheet to import and upload the file again.',
    )
  })

  it('names what existing records are matched by for each type', () => {
    expect(real('import-options-match-label', { type: 'assets' })).toBe('Match existing assets by')
    expect(real('import-options-match-label', { type: 'users' })).toBe('Match existing users by')
    expect(real('import-options-match-label', { type: 'tickets' })).toBe('Existing tickets')
  })
})
