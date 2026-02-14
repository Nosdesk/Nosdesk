export function docUrl(page: { slug?: string | null; id: string | number }): string {
  if (page.slug) return `/documentation/${page.slug}`;
  return `/documentation/${page.id}`;
}
