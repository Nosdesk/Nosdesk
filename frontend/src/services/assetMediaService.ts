import apiClient from './apiConfig';
import type { AssetMedia } from '@/types/asset';

/** Pinia Colada cache keys for an asset's media list. */
export const assetMediaKeys = {
  root: ['asset-media'] as const,
  forAsset: (assetId: number) => ['asset-media', assetId] as const,
};

export const assetMediaService = {
  async list(assetId: number): Promise<AssetMedia[]> {
    const { data } = await apiClient.get<AssetMedia[]>(`/assets/${assetId}/media`);
    return data;
  },

  async upload(assetId: number, files: File[]): Promise<AssetMedia[]> {
    const form = new FormData();
    for (const file of files) {
      form.append('files', file);
    }
    const { data } = await apiClient.post<AssetMedia[]>(`/assets/${assetId}/media`, form, {
      headers: { 'Content-Type': 'multipart/form-data' },
    });
    return data;
  },

  async update(
    assetId: number,
    mediaId: number,
    body: { caption?: string | null; sort_order?: number },
  ): Promise<AssetMedia> {
    const { data } = await apiClient.put<AssetMedia>(`/assets/${assetId}/media/${mediaId}`, body);
    return data;
  },

  async delete(assetId: number, mediaId: number): Promise<void> {
    await apiClient.delete(`/assets/${assetId}/media/${mediaId}`);
  },
};
