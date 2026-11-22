// Workspace admins' management of asset record types. The record form reads
// types through `assetRecordService.types`.
import apiClient from '../apiClient';
import type {
  AdminAssetRecordType,
  AssetRecordCoreField,
  AssetRecordCoreFieldMode,
  AssetRecordOutcome,
  AssetRecordType,
} from '../types/asset';

/** Pinia Colada cache keys for the admin list. */
export const assetRecordTypeKeys = {
  root: ['admin-asset-record-types'] as const,
  one: (id: number) => ['admin-asset-record-types', id] as const,
};

export interface CreateAssetRecordTypeBody {
  slug: string;
  label: string;
  description?: string | null;
  icon?: string | null;
  /** `#rrggbb`. */
  color?: string | null;
  field_schema?: Record<string, unknown>;
  field_order?: string[];
  core_fields?: Partial<Record<AssetRecordCoreField, AssetRecordCoreFieldMode>>;
  outcomes?: AssetRecordOutcome[];
  /** Kind slugs offered the type; null for every kind. */
  applies_to_kinds?: string[] | null;
  opens_with_status?: string | null;
  sort_order?: number;
}

/** A field left out keeps its value; `null` clears it, and `label: null`
 *  puts a built-in back to its catalogue name. */
export interface UpdateAssetRecordTypeBody {
  label?: string | null;
  description?: string | null;
  icon?: string | null;
  color?: string | null;
  field_schema?: Record<string, unknown>;
  field_order?: string[];
  core_fields?: Partial<Record<AssetRecordCoreField, AssetRecordCoreFieldMode>>;
  outcomes?: AssetRecordOutcome[];
  applies_to_kinds?: string[] | null;
  opens_with_status?: string | null;
  sort_order?: number;
}

export const assetRecordTypeService = {
  /** Every type, archived ones too, with how many records use each. */
  async list(): Promise<AdminAssetRecordType[]> {
    const { data } = await apiClient.get<AdminAssetRecordType[]>('/admin/asset-record-types');
    return data;
  },

  async get(id: number): Promise<AdminAssetRecordType> {
    const { data } = await apiClient.get<AdminAssetRecordType>(`/admin/asset-record-types/${id}`);
    return data;
  },

  async create(body: CreateAssetRecordTypeBody): Promise<AssetRecordType> {
    const { data } = await apiClient.post<AssetRecordType>('/admin/asset-record-types', body);
    return data;
  },

  /** A field or outcome change some records no longer fit answers 409
   *  `RECORDS_WOULD_NOT_FIT` with `invalid_count` and a `sample` (each the
   *  record `id`, `asset_id`, `asset_name`, `asset_tag` and `error`),
   *  unless `force`. Answers with the type and its `record_count`. */
  async update(
    id: number,
    body: UpdateAssetRecordTypeBody,
    force = false,
  ): Promise<AdminAssetRecordType> {
    const { data } = await apiClient.put<AdminAssetRecordType>(`/admin/asset-record-types/${id}`, body, {
      params: force ? { force: true } : undefined,
    });
    return data;
  },

  async archive(id: number): Promise<AdminAssetRecordType> {
    const { data } = await apiClient.post<AdminAssetRecordType>(
      `/admin/asset-record-types/${id}/archive`,
    );
    return data;
  },

  async restore(id: number): Promise<AdminAssetRecordType> {
    const { data } = await apiClient.post<AdminAssetRecordType>(
      `/admin/asset-record-types/${id}/restore`,
    );
    return data;
  },

  /** Delete an unused type of the workspace's own; archive the others. */
  async delete(id: number): Promise<void> {
    await apiClient.delete(`/admin/asset-record-types/${id}`);
  },
};
