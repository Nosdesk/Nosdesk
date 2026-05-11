/**
 * Entity types that can be searched
 */
export type SearchEntityType =
  | 'ticket'
  | 'comment'
  | 'documentation'
  | 'attachment'
  | 'device'
  | 'user';

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
}

import type { IconName } from '@/components/common/icons';

/**
 * Entity type metadata — single source of truth for display properties.
 * The `key` maps to the GroupedSearchResults property name. The
 * `icon` is a registry name resolved via the shared `<Icon>`
 * component, never a raw path.
 */
export const ENTITY_TYPE_CONFIG: Record<SearchEntityType, {
  key: keyof GroupedSearchResults;
  label: string;
  icon: IconName;
}> = {
  ticket:        { key: 'tickets',       label: 'Tickets',        icon: 'ticket' },
  comment:       { key: 'comments',      label: 'Comments',       icon: 'comment' },
  documentation: { key: 'documentation', label: 'Documentation',  icon: 'document' },
  attachment:    { key: 'attachments',   label: 'Attachments',    icon: 'paperclip' },
  device:        { key: 'devices',       label: 'Devices',        icon: 'device' },
  user:          { key: 'users',         label: 'Users',          icon: 'user' },
};

/** Display order for search result groups */
export const ENTITY_DISPLAY_ORDER: SearchEntityType[] = [
  'ticket', 'documentation', 'device', 'user', 'comment', 'attachment',
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
  return ENTITY_TYPE_CONFIG[type]?.label ?? type;
}
