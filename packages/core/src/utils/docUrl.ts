export function docUrl(page: { slug?: string | null; id: string | number }): string {
  if (page.slug) return `/documentation/${page.slug}`;
  return `/documentation/${page.id}`;
}

export function slugify(text: string): string {
  return text.toLowerCase().replace(/\s+/g, '-').replace(/[^a-z0-9-]/g, '')
}
