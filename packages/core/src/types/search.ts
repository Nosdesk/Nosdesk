/**
 * Entity types that can be searched
 */
export type SearchEntityType =
  | 'ticket'
  | 'comment'
  | 'documentation'
  | 'attachment'
  | 'device'
  | 'user'
  | 'project';

/**
 * A single search result
 */
export interface SearchResult {
  id: string;
  entity_type: SearchEntityType;
  entity_id: number;
  title: string;
  preview: string;
  url: string;
  score: number;
  updated_at?: string;
  /** Only set for `entity_type === "comment"`; true means the hit
   *  is an internal note. Drives the per-row "Internal" badge so
   *  staff can tell at a glance that a search hit came from a
   *  working note rather than the public conversation. Non-staff
   *  callers never see these results (the backend filters them
   *  out before they reach the wire). */
  is_internal?: boolean;
}

/**
 * Search API response
 */
export interface SearchResponse {
  results: SearchResult[];
  total: number;
  query: string;
  took_ms: number;
}

/**
 * Search query parameters
 */
export interface SearchParams {
  q: string;
  limit?: number;
  types?: string;
}

/**
 * Index statistics (admin only)
 */
export interface IndexStats {
  total_documents: number;
  by_type: Record<string, number>;
  index_size_bytes: number;
  is_rebuilding: boolean;
}

/**
 * Rebuild response
 */
export interface RebuildResponse {
  success: boolean;
  message: string;
  stats: {
    tickets: number;
    comments: number;
    documentation: number;
    attachments: number;
    devices: number;
    users: number;
    projects: number;
    total: number;
  };
}

/**
 * Results grouped by entity type for display
 */
export interface GroupedSearchResults {
  tickets: SearchResult[];
  comments: SearchResult[];
  documentation: SearchResult[];
  attachments: SearchResult[];
  devices: SearchResult[];
  users: SearchResult[];
  projects: SearchResult[];
}

import { translate } from '../i18n';

// A frontend icon-registry key (@/components/common/icons in the web app).
// Loosened to `string` here so @nosdesk/core stays headless; the host's
// `<Icon>` component resolves and validates the name at render.
type IconName = string;

/**
 * Entity type metadata, single source of truth for display
 * properties. The `key` maps to the GroupedSearchResults property
 * name. The `icon` is a registry name resolved via the shared
 * `<Icon>` component, never a raw path. `labelKey` is a Fluent
 * key, resolved at render time via `translate()` (see
 * `getEntityTypeLabel`).
 */
export const ENTITY_TYPE_CONFIG: Record<SearchEntityType, {
  key: keyof GroupedSearchResults;
  labelKey: string;
  labelFallback: string;
  icon: IconName;
}> = {
  ticket:        { key: 'tickets',       labelKey: 'search-entity-type-tickets',       labelFallback: 'Tickets',       icon: 'ticket' },
  comment:       { key: 'comments',      labelKey: 'search-entity-type-comments',      labelFallback: 'Comments',      icon: 'comment' },
  documentation: { key: 'documentation', labelKey: 'search-entity-type-documentation', labelFallback: 'Documentation', icon: 'document' },
  attachment:    { key: 'attachments',   labelKey: 'search-entity-type-attachments',   labelFallback: 'Attachments',   icon: 'paperclip' },
  device:        { key: 'devices',       labelKey: 'search-entity-type-devices',       labelFallback: 'Devices',       icon: 'device' },
  user:          { key: 'users',         labelKey: 'search-entity-type-users',         labelFallback: 'Users',         icon: 'user' },
  project:       { key: 'projects',      labelKey: 'search-entity-type-projects',      labelFallback: 'Projects',      icon: 'folder' },
};

/** Display order for search result groups */
export const ENTITY_DISPLAY_ORDER: SearchEntityType[] = [
  'ticket', 'project', 'documentation', 'device', 'user', 'comment', 'attachment',
];

/** Create an empty GroupedSearchResults object */
export function emptyGroupedResults(): GroupedSearchResults {
  return {
    tickets: [],
    comments: [],
    documentation: [],
    attachments: [],
    devices: [],
    users: [],
    projects: [],
  };
}

/** Group a flat list of results by entity type */
export function groupResultsByType(results: SearchResult[]): GroupedSearchResults {
  const grouped = emptyGroupedResults();
  for (const result of results) {
    const key = ENTITY_TYPE_CONFIG[result.entity_type]?.key;
    if (key) grouped[key].push(result);
  }
  return grouped;
}

/** Get display label for an entity type */
export function getEntityTypeLabel(type: SearchEntityType): string {
  const cfg = ENTITY_TYPE_CONFIG[type];
  if (!cfg) return type;
  return translate(cfg.labelKey, undefined, cfg.labelFallback);
}
