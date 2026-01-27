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

/**
 * Entity type metadata — single source of truth for display properties.
 * The `key` maps to the GroupedSearchResults property name.
 */
export const ENTITY_TYPE_CONFIG: Record<SearchEntityType, {
  key: keyof GroupedSearchResults;
  label: string;
  icon: string;
}> = {
  ticket:        { key: 'tickets',       label: 'Tickets',        icon: 'M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4' },
  comment:       { key: 'comments',      label: 'Comments',       icon: 'M7 8h10M7 12h4m1 8l-4-4H5a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v8a2 2 0 01-2 2h-3l-4 4z' },
  documentation: { key: 'documentation', label: 'Documentation',  icon: 'M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z' },
  attachment:    { key: 'attachments',   label: 'Attachments',    icon: 'M15.172 7l-6.586 6.586a2 2 0 102.828 2.828l6.414-6.586a4 4 0 00-5.656-5.656l-6.415 6.585a6 6 0 108.486 8.486L20.5 13' },
  device:        { key: 'devices',       label: 'Devices',        icon: 'M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z' },
  user:          { key: 'users',         label: 'Users',          icon: 'M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z' },
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
