/**
 * The type detection line on Map: whether a file's headers look like
 * another import type than the one chosen at Start, and which headers say
 * so. Headers are compared the way the server matches them
 * (`mapping::normalize`): letters and digits only, lower case, `#` read as
 * "no".
 */
import type { ImportJobType } from '@nosdesk/core/services/importService'

export interface TypeHint {
  /** The type the headers look like. */
  type: ImportJobType
  /** The file's headers that suggest it, in file order. */
  headers: string[]
}

export function normalizeHeader(header: string): string {
  let out = ''
  for (const c of header) {
    if (c === '#') out += 'no'
    else if (/[\p{L}\p{N}]/u.test(c)) out += c.toLowerCase()
  }
  return out
}

const EMAIL = new Set(['email', 'emailaddress', 'mail', 'workemail'])
const PERSON_NAME = new Set([
  'first',
  'firstname',
  'givenname',
  'last',
  'lastname',
  'surname',
  'familyname',
])
const ASSET_KEY = new Set([
  'tag',
  'tagno',
  'assettag',
  'assetno',
  'assetnumber',
  'serial',
  'serialno',
  'serialnumber',
  'sn',
])
const ASSET_NAME = new Set(['name', 'assetname', 'devicename', 'itemname'])

/**
 * The type `headers` look like when it isn't `current`, or null.
 *
 * Users: an email column and a first or last name column, and nothing
 * that keys an asset. Assets: an asset tag or serial column and a name
 * column. Tickets are never guessed; their columns overlap the others.
 */
export function detectOtherType(current: ImportJobType, headers: string[]): TypeHint | null {
  const pick = (set: Set<string>) => headers.filter((h) => set.has(normalizeHeader(h)))
  const emails = pick(EMAIL)
  const names = pick(PERSON_NAME)
  const keys = pick(ASSET_KEY)

  if (current !== 'users' && emails.length > 0 && names.length > 0 && keys.length === 0) {
    const used = new Set([emails[0], ...names])
    return { type: 'users', headers: headers.filter((h) => used.has(h)) }
  }
  const assetNames = pick(ASSET_NAME)
  if (current !== 'assets' && keys.length > 0 && assetNames.length > 0) {
    const used = new Set([...keys, assetNames[0]])
    return { type: 'assets', headers: headers.filter((h) => used.has(h)) }
  }
  return null
}

/** Headers as a sentence list in the page's language: "Email, First and Last". */
export function listHeaders(headers: string[], locale?: string): string {
  try {
    return new Intl.ListFormat(locale || undefined, { style: 'long', type: 'conjunction' }).format(
      headers,
    )
  } catch {
    return headers.join(', ')
  }
}
