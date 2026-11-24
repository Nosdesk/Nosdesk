// Asset records: the work done on an asset, and the record types and
// vendors the record form offers. Staff only.
import apiClient from '../apiClient';
import type {
  AssetMedia,
  AssetRecord,
  AssetRecordCatalogueEntry,
  AssetRecordPage,
  AssetRecordSummary,
  AssetRecordType,
  AssetRecordTypeList,
  AssetRecordWrite,
} from '../types/asset';

/** Pinia Colada cache keys for records and their types. */
export const assetRecordKeys = {
  root: ['asset-records'] as const,
  forAsset: (assetId: number) => ['asset-records', assetId] as const,
  summary: (assetId: number, recordType?: string) =>
    ['asset-records', assetId, 'summary', recordType ?? ''] as const,
  types: ['asset-record-types'] as const,
  catalogue: ['asset-record-types', 'catalogue'] as const,
  vendors: (query: string) => ['asset-record-vendors', query] as const,
};

/** A record type by id or slug. */
export type AssetRecordTypeRef = number | string;

/** A cost as written: a decimal string with `.` as the mark. Without a
 *  currency it is in the workspace currency. */
export interface AssetRecordCostInput {
  amount: string;
  currency?: string | null;
}

/** A record to write. Give `occurred_on` (a day) or `occurred_at` (a
 *  moment), and likewise for completion; leave completion out while the
 *  work is open. Sending `holder_uuid` or `location`, even as null, records
 *  them as entered; sending neither derives them from the asset's loans. */
export interface CreateAssetRecordBody {
  record_type: AssetRecordTypeRef;
  summary?: string | null;
  notes?: string | null;
  occurred_on?: string;
  occurred_at?: string;
  completed_on?: string | null;
  completed_at?: string | null;
  outcome?: string | null;
  vendor?: string | null;
  performed_by_uuid?: string | null;
  reference?: string | null;
  cost?: AssetRecordCostInput | null;
  ticket_id?: number | null;
  replacement_asset_id?: number | null;
  holder_uuid?: string | null;
  location?: string | null;
  attributes?: Record<string, unknown>;
  /** The source system's id, which makes the write idempotent. */
  source_ref?: string | null;
}

/** An edit to the record as it stood at `expected_version`. A field left
 *  out keeps its value; `null` clears it. */
export interface UpdateAssetRecordBody {
  expected_version: number;
  record_type?: AssetRecordTypeRef;
  summary?: string | null;
  notes?: string | null;
  occurred_on?: string;
  occurred_at?: string;
  /** `null` reopens the work. */
  completed_on?: string | null;
  completed_at?: string | null;
  outcome?: string | null;
  vendor?: string | null;
  performed_by_uuid?: string | null;
  reference?: string | null;
  cost?: AssetRecordCostInput | null;
  ticket_id?: number | null;
  replacement_asset_id?: number | null;
  holder_uuid?: string | null;
  location?: string | null;
  attributes?: Record<string, unknown>;
}

/** Which of an asset's records a page lists. */
export interface AssetRecordFilter {
  /** A type's slug or id. */
  type?: AssetRecordTypeRef;
  outcome?: string;
  /** `true` for open work only, `false` for completed only. */
  open?: boolean;
  /** Only records a move out of this status completes: of the type that
   *  opens with it, or opened by a move into it. */
  leaving?: string;
  /** Work done on or after this day (YYYY-MM-DD). */
  from?: string;
  /** Work done on or before this day (YYYY-MM-DD). */
  to?: string;
  /** The `next_cursor` of the page before. */
  before?: string;
  limit?: number;
}

export const assetRecordService = {
  async list(assetId: number, filter: AssetRecordFilter = {}): Promise<AssetRecordPage> {
    const { data } = await apiClient.get<AssetRecordPage>(`/assets/${assetId}/records`, {
      params: filter,
    });
    return data;
  },

  async get(assetId: number, recordId: number): Promise<AssetRecord> {
    const { data } = await apiClient.get<AssetRecord>(`/assets/${assetId}/records/${recordId}`);
    return data;
  },

  /** What the asset's records add up to, of one type when `recordType`
   *  (a slug or id) is given. */
  async summary(assetId: number, recordType?: AssetRecordTypeRef): Promise<AssetRecordSummary> {
    const { data } = await apiClient.get<AssetRecordSummary>(
      `/assets/${assetId}/records/summary`,
      { params: recordType === undefined || recordType === '' ? undefined : { record_type: recordType } },
    );
    return data;
  },

  async create(assetId: number, body: CreateAssetRecordBody): Promise<AssetRecordWrite> {
    const { data } = await apiClient.post<AssetRecordWrite>(`/assets/${assetId}/records`, body);
    return data;
  },

  async update(
    assetId: number,
    recordId: number,
    body: UpdateAssetRecordBody,
  ): Promise<AssetRecordWrite> {
    const { data } = await apiClient.patch<AssetRecordWrite>(
      `/assets/${assetId}/records/${recordId}`,
      body,
    );
    return data;
  },

  /** Delete a record. Its files stay on the asset unless `deleteFiles`. */
  async delete(
    assetId: number,
    recordId: number,
    deleteFiles = false,
  ): Promise<{ deleted: boolean; files_deleted: number }> {
    const { data } = await apiClient.delete<{ deleted: boolean; files_deleted: number }>(
      `/assets/${assetId}/records/${recordId}`,
      { params: deleteFiles ? { delete_files: true } : undefined },
    );
    return data;
  },

  /** Make `mediaIds`, all files of the asset, the record's files. */
  async setFiles(assetId: number, recordId: number, mediaIds: number[]): Promise<AssetMedia[]> {
    const { data } = await apiClient.put<AssetMedia[]>(
      `/assets/${assetId}/records/${recordId}/media`,
      { media_ids: mediaIds },
    );
    return data;
  },

  /** Vendors already written on records, most used first. */
  async vendors(query = '', limit?: number): Promise<string[]> {
    const { data } = await apiClient.get<string[]>('/assets/record-vendors', {
      params: { q: query || undefined, limit },
    });
    return data;
  },

  /** The workspace's types for the record form, with the currency its cost
   *  field offers. */
  async types(includeArchived = false): Promise<AssetRecordTypeList> {
    const { data } = await apiClient.get<AssetRecordTypeList>('/assets/record-types', {
      params: includeArchived ? { include_archived: true } : undefined,
    });
    return data;
  },

  /** The built-in types, each marked as added or not. */
  async catalogue(): Promise<AssetRecordCatalogueEntry[]> {
    const { data } = await apiClient.get<AssetRecordCatalogueEntry[]>(
      '/assets/record-types/catalogue',
    );
    return data;
  },

  /** Add a built-in type to the workspace, or get the one already added. */
  async addFromCatalogue(slug: string): Promise<AssetRecordType> {
    const { data } = await apiClient.post<AssetRecordType>(
      `/assets/record-types/catalogue/${encodeURIComponent(slug)}`,
    );
    return data;
  },
};
