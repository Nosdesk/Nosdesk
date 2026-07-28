/**
 * Full-page plugin surface (`nav.item`).
 *
 * A `nav.item` contribution is a nav link to a route that renders the plugin's
 * component full-page (not an iframe embedded in host chrome, not a modal). This
 * is the single place the route name + path shape live, shared by the router,
 * the Navbar link injection, and the page view.
 */
export const PLUGIN_PAGE_ROUTE = 'plugin-page';

/** Route path for a plugin's full-page component. */
export function pluginPagePath(pluginUuid: string, componentName: string): string {
  return `/plugins/${pluginUuid}/pages/${encodeURIComponent(componentName)}`;
}
