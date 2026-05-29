/**
 * Bulk holiday-import presets for the SLA calendar editor.
 *
 * Each template generates a list of `HolidayPreset` rows for one
 * country: fixed-date holidays use `recurrence: 'annual'` so the
 * engine's MM-DD expansion (`repository::sla::expand_holiday`)
 * handles every future year for free; variable-date holidays
 * (Easter-derived in every country; Monday-rule federal holidays
 * in the US) are computed per year and emitted as
 * `recurrence: 'none'` rows for a 3-year coverage window (current
 * year + two future years).
 *
 * A 3-year window means re-importing the template once every few
 * years to refresh the variable dates. The picker UI surfaces the
 * window in its label so the admin knows what's covered.
 *
 * Scope is the country-level public/federal/national holiday set.
 * State and regional variations (e.g. Queen's/King's Birthday in
 * Australia, Scotland-only bank holidays in the UK, Alsace-Moselle
 * in France) are intentionally out of scope for v1 — they're
 * one-off additions the admin can make manually.
 *
 * Holiday `label` strings are written in the country's primary
 * language because the label is admin-readable context, not
 * user-facing UI copy. A French admin importing AU holidays will
 * still see "Good Friday" on the row, which reads as more
 * faithful to the source than a forced translation.
 */

export type CountryCode = 'AU' | 'US' | 'UK' | 'NL' | 'FR'

export interface HolidayPreset {
  /** ISO YYYY-MM-DD. For annual recurrence the year is informative
   *  only — the engine matches on MM-DD. */
  date: string
  label: string
  recurrence: 'annual' | 'none'
}

export interface HolidayTemplate {
  code: CountryCode
  /** Display name shown in the picker; English by design. */
  name: string
  generate(currentYear: number): HolidayPreset[]
}

// ---------------- Date helpers ----------------

function pad(n: number): string {
  return String(n).padStart(2, '0')
}

function isoDate(year: number, month: number, day: number): string {
  return `${year}-${pad(month)}-${pad(day)}`
}

interface YMD {
  year: number
  month: number
  day: number
}

function ymd(year: number, month: number, day: number): YMD {
  return { year, month, day }
}

function addDays(base: YMD, offset: number): YMD {
  const d = new Date(Date.UTC(base.year, base.month - 1, base.day + offset))
  return {
    year: d.getUTCFullYear(),
    month: d.getUTCMonth() + 1,
    day: d.getUTCDate(),
  }
}

/**
 * Gauss / Meeus algorithm for Western (Gregorian) Easter Sunday.
 * Verified: 2024 -> Mar 31, 2025 -> Apr 20, 2026 -> Apr 5,
 * 2027 -> Mar 28, 2028 -> Apr 16.
 */
function easterSunday(year: number): YMD {
  const a = year % 19
  const b = Math.floor(year / 100)
  const c = year % 100
  const d = Math.floor(b / 4)
  const e = b % 4
  const f = Math.floor((b + 8) / 25)
  const g = Math.floor((b - f + 1) / 3)
  const h = (19 * a + b - d - g + 15) % 30
  const i = Math.floor(c / 4)
  const k = c % 4
  const l = (32 + 2 * e + 2 * i - h - k) % 7
  const m = Math.floor((a + 11 * h + 22 * l) / 451)
  const month = Math.floor((h + l - 7 * m + 114) / 31)
  const day = ((h + l - 7 * m + 114) % 31) + 1
  return { year, month, day }
}

/**
 * Nth occurrence of `weekday` in `month` of `year`.
 * `weekday`: 0 = Sun ... 6 = Sat. `n`: 1 = first, 2 = second...
 */
function nthWeekdayOfMonth(year: number, month: number, weekday: number, n: number): YMD {
  const firstOfMonth = new Date(Date.UTC(year, month - 1, 1))
  const firstWeekday = firstOfMonth.getUTCDay()
  const offset = (weekday - firstWeekday + 7) % 7
  const day = 1 + offset + (n - 1) * 7
  return { year, month, day }
}

/** Last occurrence of `weekday` in `month` of `year`. */
function lastWeekdayOfMonth(year: number, month: number, weekday: number): YMD {
  // Day 0 of next month = last day of this month.
  const lastOfMonth = new Date(Date.UTC(year, month, 0))
  const lastDay = lastOfMonth.getUTCDate()
  const lastWeekday = lastOfMonth.getUTCDay()
  const offset = (lastWeekday - weekday + 7) % 7
  return { year, month, day: lastDay - offset }
}

/** Range used for emitting variable-date (`recurrence: 'none'`)
 *  holiday rows. Current year + the next two. */
const VARIABLE_YEAR_WINDOW = 3

function variableYears(currentYear: number): number[] {
  return Array.from({ length: VARIABLE_YEAR_WINDOW }, (_, i) => currentYear + i)
}

function preset(date: YMD, label: string, recurrence: 'annual' | 'none'): HolidayPreset {
  return { date: isoDate(date.year, date.month, date.day), label, recurrence }
}

// ---------------- Templates ----------------

/** Australia — national public holidays observed in all states.
 *  Excludes state-specific (Queen's/King's Birthday, Labour Day,
 *  Melbourne Cup) and Easter Saturday/Sunday which vary by state. */
function australia(currentYear: number): HolidayPreset[] {
  const out: HolidayPreset[] = [
    preset(ymd(currentYear, 1, 1), 'New Year\'s Day', 'annual'),
    preset(ymd(currentYear, 1, 26), 'Australia Day', 'annual'),
    preset(ymd(currentYear, 4, 25), 'ANZAC Day', 'annual'),
    preset(ymd(currentYear, 12, 25), 'Christmas Day', 'annual'),
    preset(ymd(currentYear, 12, 26), 'Boxing Day', 'annual'),
  ]
  for (const year of variableYears(currentYear)) {
    const easter = easterSunday(year)
    out.push(preset(addDays(easter, -2), 'Good Friday', 'none'))
    out.push(preset(addDays(easter, 1), 'Easter Monday', 'none'))
  }
  return out
}

/** United States — federal holidays per 5 U.S.C. § 6103.
 *  Excludes Columbus Day (states diverge on Indigenous Peoples'
 *  Day vs Columbus Day observance). */
function unitedStates(currentYear: number): HolidayPreset[] {
  const out: HolidayPreset[] = [
    preset(ymd(currentYear, 1, 1), 'New Year\'s Day', 'annual'),
    preset(ymd(currentYear, 6, 19), 'Juneteenth National Independence Day', 'annual'),
    preset(ymd(currentYear, 7, 4), 'Independence Day', 'annual'),
    preset(ymd(currentYear, 11, 11), 'Veterans Day', 'annual'),
    preset(ymd(currentYear, 12, 25), 'Christmas Day', 'annual'),
  ]
  for (const year of variableYears(currentYear)) {
    out.push(preset(nthWeekdayOfMonth(year, 1, 1, 3), 'Martin Luther King Jr. Day', 'none'))
    out.push(preset(nthWeekdayOfMonth(year, 2, 1, 3), 'Presidents\' Day', 'none'))
    out.push(preset(lastWeekdayOfMonth(year, 5, 1), 'Memorial Day', 'none'))
    out.push(preset(nthWeekdayOfMonth(year, 9, 1, 1), 'Labor Day', 'none'))
    out.push(preset(nthWeekdayOfMonth(year, 11, 4, 4), 'Thanksgiving Day', 'none'))
  }
  return out
}

/** United Kingdom — bank holidays for England & Wales.
 *  Scotland diverges (2 Jan, August Bank Holiday on first Monday
 *  rather than last) and would warrant a separate template. */
function unitedKingdom(currentYear: number): HolidayPreset[] {
  const out: HolidayPreset[] = [
    preset(ymd(currentYear, 1, 1), 'New Year\'s Day', 'annual'),
    preset(ymd(currentYear, 12, 25), 'Christmas Day', 'annual'),
    preset(ymd(currentYear, 12, 26), 'Boxing Day', 'annual'),
  ]
  for (const year of variableYears(currentYear)) {
    const easter = easterSunday(year)
    out.push(preset(addDays(easter, -2), 'Good Friday', 'none'))
    out.push(preset(addDays(easter, 1), 'Easter Monday', 'none'))
    out.push(preset(nthWeekdayOfMonth(year, 5, 1, 1), 'Early May bank holiday', 'none'))
    out.push(preset(lastWeekdayOfMonth(year, 5, 1), 'Spring bank holiday', 'none'))
    out.push(preset(lastWeekdayOfMonth(year, 8, 1), 'Summer bank holiday', 'none'))
  }
  return out
}

/** Netherlands — algemeen erkende feestdagen. Labels in Dutch. */
function netherlands(currentYear: number): HolidayPreset[] {
  const out: HolidayPreset[] = [
    preset(ymd(currentYear, 1, 1), 'Nieuwjaarsdag', 'annual'),
    preset(ymd(currentYear, 4, 27), 'Koningsdag', 'annual'),
    preset(ymd(currentYear, 5, 5), 'Bevrijdingsdag', 'annual'),
    preset(ymd(currentYear, 12, 25), 'Eerste Kerstdag', 'annual'),
    preset(ymd(currentYear, 12, 26), 'Tweede Kerstdag', 'annual'),
  ]
  for (const year of variableYears(currentYear)) {
    const easter = easterSunday(year)
    out.push(preset(addDays(easter, -2), 'Goede Vrijdag', 'none'))
    out.push(preset(addDays(easter, 1), 'Tweede Paasdag', 'none'))
    out.push(preset(addDays(easter, 39), 'Hemelvaartsdag', 'none'))
    out.push(preset(addDays(easter, 50), 'Tweede Pinksterdag', 'none'))
  }
  return out
}

/** France — jours fériés nationaux. Labels in French.
 *  Excludes the Alsace-Moselle-only Good Friday and St. Stephen's
 *  Day. */
function france(currentYear: number): HolidayPreset[] {
  const out: HolidayPreset[] = [
    preset(ymd(currentYear, 1, 1), 'Jour de l\'an', 'annual'),
    preset(ymd(currentYear, 5, 1), 'Fête du Travail', 'annual'),
    preset(ymd(currentYear, 5, 8), 'Victoire 1945', 'annual'),
    preset(ymd(currentYear, 7, 14), 'Fête nationale', 'annual'),
    preset(ymd(currentYear, 8, 15), 'Assomption', 'annual'),
    preset(ymd(currentYear, 11, 1), 'Toussaint', 'annual'),
    preset(ymd(currentYear, 11, 11), 'Armistice 1918', 'annual'),
    preset(ymd(currentYear, 12, 25), 'Noël', 'annual'),
  ]
  for (const year of variableYears(currentYear)) {
    const easter = easterSunday(year)
    out.push(preset(addDays(easter, 1), 'Lundi de Pâques', 'none'))
    out.push(preset(addDays(easter, 39), 'Ascension', 'none'))
    out.push(preset(addDays(easter, 50), 'Lundi de Pentecôte', 'none'))
  }
  return out
}

export const HOLIDAY_TEMPLATES: Record<CountryCode, HolidayTemplate> = {
  AU: { code: 'AU', name: 'Australia', generate: australia },
  US: { code: 'US', name: 'United States', generate: unitedStates },
  UK: { code: 'UK', name: 'United Kingdom', generate: unitedKingdom },
  NL: { code: 'NL', name: 'Netherlands', generate: netherlands },
  FR: { code: 'FR', name: 'France', generate: france },
}

export const HOLIDAY_TEMPLATE_LIST: HolidayTemplate[] = Object.values(HOLIDAY_TEMPLATES)
