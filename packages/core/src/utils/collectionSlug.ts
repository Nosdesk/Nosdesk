/**
 * Collection slug helpers. Mirrors backend `utils/slug.rs` so
 * auto-generated slugs match what the API would produce.
 */

function collapseHyphens(value: string): string {
  let result = ''
  let prevHyphen = false
  for (const char of value) {
    if (char === '-') {
      if (!prevHyphen) result += '-'
      prevHyphen = true
    } else {
      result += char
      prevHyphen = false
    }
  }
  return result
}

/** Turn a collection title into a URL-safe slug fragment. */
export function slugifyCollectionTitle(title: string): string {
  const slug = collapseHyphens(
    title
      .toLowerCase()
      .split('')
      .map((char) => (/[a-z0-9-]/.test(char) ? char : '-'))
      .join(''),
  ).replace(/^-+|-+$/g, '')

  if (!slug) return ''
  if (/^\d+$/.test(slug)) return `page-${slug}`
  return slug
}

/** Slugify a title for auto-generation; empty titles become `untitled`. */
export function slugifyCollectionTitleOrDefault(title: string): string {
  const slug = slugifyCollectionTitle(title)
  return slug || 'untitled'
}

/** Pick the first slug in `base`, `base-2`, `base-3`, … not in `taken`. */
export function uniqueCollectionSlug(base: string, taken: ReadonlySet<string>): string {
  const normalized = base || 'untitled'
  if (!taken.has(normalized)) return normalized

  for (let n = 2; n < 1000; n += 1) {
    const candidate = `${normalized}-${n}`
    if (!taken.has(candidate)) return candidate
  }

  return `${normalized}-${Date.now()}`
}

/** Slugify a title and avoid collisions with existing collection slugs. */
export function slugFromCollectionTitle(
  title: string,
  taken: ReadonlySet<string>,
): string {
  return uniqueCollectionSlug(slugifyCollectionTitleOrDefault(title), taken)
}
