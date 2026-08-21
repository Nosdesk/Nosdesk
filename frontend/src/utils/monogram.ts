/**
 * Monogram marks for entities that may not have an uploaded image.
 *
 * Used by the workspace switcher, where most workspaces will never upload a
 * logo, so the monogram is the ordinary case rather than an error state.
 *
 * Colour is derived from a stable key (a uuid), not from the display name.
 * Keying on the first letter, as `UserAvatar` does, gives every workspace
 * starting with the same letter an identical colour, which defeats the point
 * of a mark whose job is telling two workspaces apart at a glance.
 */

/** Up to two initials, or `?` when there is nothing to take them from. */
export function initialsFrom(name: string): string {
  if (!name) return '?';
  return (
    name
      .split(/[\s-]+/)
      .filter((part) => part.length > 0)
      .map((word) => word.charAt(0))
      .join('')
      .toUpperCase()
      .slice(0, 2) || '?'
  );
}

/**
 * A stable hue in [0, 360) for `key`.
 *
 * FNV-1a over the whole key, so two keys sharing a prefix (uuids often share
 * their first characters) still land in different places on the wheel.
 */
export function monogramHue(key: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < key.length; i++) {
    hash ^= key.charCodeAt(i);
    // FNV prime, via shifts to stay in 32-bit range without Math.imul overflow.
    hash = (hash + ((hash << 1) + (hash << 4) + (hash << 7) + (hash << 8) + (hash << 24))) >>> 0;
  }
  return hash % 360;
}

/** The `hsl()` background for a monogram. Lightness is fixed so white text
 *  keeps its contrast in both themes. */
export function monogramColor(key: string): string {
  return `hsl(${monogramHue(key)}, 65%, 38%)`;
}

/** XML-escape text destined for an SVG data URI. */
function escapeXml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&apos;');
}

/**
 * The monogram as an SVG data URI, for slots that take an image URL rather
 * than markup (`MenuItem.iconUrl`). Rendered in an `<img>`, so the SVG is
 * inert regardless of what the name contains, and escaped anyway so a name
 * with a quote or an ampersand cannot break the document.
 */
export function monogramDataUri(name: string, key: string): string {
  const initials = escapeXml(initialsFrom(name));
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">` +
    `<rect width="32" height="32" rx="7" fill="${monogramColor(key)}"/>` +
    `<text x="16" y="17" fill="#fff" font-family="system-ui,sans-serif" font-size="14"` +
    ` font-weight="600" text-anchor="middle" dominant-baseline="central">${initials}</text>` +
    `</svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}
