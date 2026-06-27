import apiClient from '../apiClient';

export type FeatureFlagValue = boolean | string | number | null | Record<string, unknown>;
export type FeatureFlagMap = Record<string, FeatureFlagValue>;

export const featureFlagsService = {
  async getMine(): Promise<FeatureFlagMap> {
    const { data } = await apiClient.get<FeatureFlagMap>('/feature-flags');
    return data;
  },

  async patchWorkspace(flag: string, value: FeatureFlagValue | undefined): Promise<FeatureFlagMap> {
    const { data } = await apiClient.patch<FeatureFlagMap>('/admin/feature-flags', {
      flag,
      value: value === undefined ? null : value,
    });
    return data;
  },

  async replaceWorkspace(flags: FeatureFlagMap): Promise<FeatureFlagMap> {
    const { data } = await apiClient.put<FeatureFlagMap>('/admin/feature-flags', { flags });
    return data;
  },

  async patchUserOverride(
    userUuid: string,
    flag: string,
    value: FeatureFlagValue | undefined,
  ): Promise<FeatureFlagMap> {
    const { data } = await apiClient.patch<FeatureFlagMap>(
      `/admin/feature-flags/users/${encodeURIComponent(userUuid)}`,
      { flag, value: value === undefined ? null : value },
    );
    return data;
  },
};
