<script setup lang="ts">
/**
 * Tenant self-serve workspace member management (P1.3).
 *
 * Lists the current workspace's members and lets an owner/admin change
 * roles or remove members. Acts on the caller's own workspace via the
 * context-scoped `/workspace/members` endpoints (no workspace id), so a
 * workspace admin can only ever manage their own team.
 *
 * The UI mirrors the backend's tiered authorization: owners manage every
 * role; admins manage only agents and members and can assign only
 * agent/member. Rows the caller can't manage render read-only. The
 * backend re-checks everything, so this is convenience, not the boundary.
 */
import { computed, ref } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import { formatDistanceToNow } from 'date-fns';

import AlertMessage from '@/components/common/AlertMessage.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Button from '@/components/common/Button.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Icon from '@/components/common/Icon.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import workspacesService from '@/services/workspacesService';
import { useAuthStore } from '@/stores/auth';
import type { WorkspaceMember, WorkspaceRole } from '@/types/workspace';
import type { User } from '@/types/user';
import { extractErrorMessage } from '@/utils/errors';
import * as syncPool from '@/sync/pool';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const router = useRouter();
const auth = useAuthStore();

const WORKSPACE_ROLES: WorkspaceRole[] = ['owner', 'admin', 'agent', 'member'];
const ROLE_RANK: Record<WorkspaceRole, number> = { member: 0, agent: 1, admin: 2, owner: 3 };

const MEMBERS_KEY = ['workspace-members'] as const;
const queryCache = useQueryCache();

const membersQuery = useQuery({
  key: MEMBERS_KEY,
  query: () => workspacesService.listWorkspaceMembers(),
});

const members = computed<WorkspaceMember[]>(() =>
  Array.isArray(membersQuery.data.value) ? membersQuery.data.value : [],
);
const isFirstLoad = computed(
  () =>
    membersQuery.status.value === 'pending' && membersQuery.data.value === undefined,
);
const loadError = computed(() =>
  membersQuery.error.value ? t('admin-workspace-members-error-load') : '',
);

const ownerCount = computed(() => members.value.filter((m) => m.role === 'owner').length);

// The caller's role in this workspace drives which rows are editable.
const myRole = computed<WorkspaceRole>(() => auth.user?.workspace_role ?? 'member');
const myUuid = computed(() => auth.user?.uuid ?? '');

const isSaving = ref(false);
const errorMessage = ref('');
const memberToRemove = ref<WorkspaceMember | null>(null);

const refresh = () => queryCache.invalidateQueries({ key: MEMBERS_KEY });

/** Owners manage every role; admins only agents/members. */
function canManage(targetRole: WorkspaceRole): boolean {
  if (myRole.value === 'owner') return true;
  if (myRole.value === 'admin') return ROLE_RANK[targetRole] < ROLE_RANK.admin;
  return false;
}
/** Which roles the caller may assign (same tier as canManage). */
function canAssign(role: WorkspaceRole): boolean {
  if (myRole.value === 'owner') return true;
  if (myRole.value === 'admin') return ROLE_RANK[role] < ROLE_RANK.admin;
  return false;
}
function isSoleOwner(member: WorkspaceMember): boolean {
  return member.role === 'owner' && ownerCount.value === 1;
}
/** A row is editable when the caller's tier can manage it and it isn't
 *  the last owner (whom no one may demote/remove). */
function canEditRow(member: WorkspaceMember): boolean {
  return canManage(member.role) && !isSoleOwner(member);
}

function isLastOwnerError(error: unknown): boolean {
  const e = error as { response?: { status?: number; data?: { error?: string } } };
  return e.response?.status === 409 && e.response?.data?.error === 'last_owner';
}
function isForbidden(error: unknown): boolean {
  return (error as { response?: { status?: number } }).response?.status === 403;
}
function resolveMutationError(error: unknown, fallbackKey: string): string {
  if (isLastOwnerError(error)) return t('admin-workspace-members-last-owner-hint');
  if (isForbidden(error)) return t('workspace-members-error-forbidden');
  return extractErrorMessage(error, t(fallbackKey));
}

function memberDisplayName(uuid: string): string {
  const cached = syncPool.get('user', uuid) as Pick<User, 'name' | 'email'> | undefined;
  return cached?.name ?? cached?.email ?? uuid;
}
function roleLabel(role: WorkspaceRole): string {
  return t(`admin-workspace-members-role-${role}`);
}

/** Per-row role options. Roles the caller can't grant are disabled,
 *  except the member's current role (always shown so the value reads
 *  correctly). */
function roleOptionsFor(member: WorkspaceMember) {
  return WORKSPACE_ROLES.map((role) => ({
    value: role,
    label: roleLabel(role),
    disabled: !canAssign(role) && role !== member.role,
  }));
}
function formatWhen(iso: string | null): string {
  if (!iso) return t('admin-workspace-members-accepted-pending');
  try {
    return formatDistanceToNow(new Date(iso), { addSuffix: true });
  } catch {
    return iso;
  }
}

async function changeRole(userUuid: string, role: WorkspaceRole) {
  isSaving.value = true;
  errorMessage.value = '';
  try {
    await workspacesService.updateWorkspaceMemberRole(userUuid, { role });
    refresh();
  } catch (e: unknown) {
    errorMessage.value = resolveMutationError(e, 'admin-workspace-members-error-role');
    refresh();
  } finally {
    isSaving.value = false;
  }
}
async function removeMember(userUuid: string) {
  isSaving.value = true;
  errorMessage.value = '';
  try {
    await workspacesService.removeWorkspaceMember(userUuid);
    memberToRemove.value = null;
    refresh();
  } catch (e: unknown) {
    errorMessage.value = resolveMutationError(e, 'admin-workspace-members-error-remove');
  } finally {
    isSaving.value = false;
  }
}

function onRoleChange(member: WorkspaceMember, value: string) {
  const newRole = value as WorkspaceRole;
  if (newRole === member.role || !canEditRow(member)) return;
  void changeRole(member.user_uuid, newRole);
}
function requestRemove(member: WorkspaceMember) {
  if (!canEditRow(member)) return;
  memberToRemove.value = member;
}
function confirmRemove() {
  const member = memberToRemove.value;
  if (member) void removeMember(member.user_uuid);
}
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <header class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 class="text-xl sm:text-2xl font-bold text-primary">
            {{ $t('workspace-members-title') }}
          </h1>
          <p class="text-secondary text-sm mt-1">
            {{ $t('workspace-members-subtitle') }}
          </p>
        </div>
        <Button icon="user" variant="secondary" @click="router.push('/users')">
          {{ $t('workspace-members-invite') }}
        </Button>
      </header>

      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />
      <AlertMessage
        v-if="loadError && members.length === 0"
        type="error"
        :message="loadError"
      />

      <Skeleton
        v-if="isFirstLoad"
        :label="$t('admin-workspace-members-loading')"
        class="flex flex-col gap-2"
      >
        <div
          v-for="n in 4"
          :key="n"
          class="bg-surface border border-default rounded-lg p-3 flex items-center gap-3"
        >
          <SkeletonBar class="h-8 w-8 rounded-full shrink-0" />
          <SkeletonBar class="h-3 w-40 flex-1" />
          <SkeletonBar class="h-3 w-24" />
        </div>
      </Skeleton>

      <template v-else>
        <EmptyState
          v-if="members.length === 0"
          icon="users"
          :title="$t('empty-workspace-members-title')"
          :description="$t('workspace-members-empty-description')"
          variant="card"
        />

        <div
          v-else
          class="bg-surface border border-default rounded-xl overflow-hidden"
        >
          <div class="overflow-x-auto">
            <table class="w-full text-sm">
              <thead class="bg-surface-alt text-tertiary">
                <tr>
                  <th class="text-left px-3 py-2 font-medium">
                    {{ $t('admin-workspace-members-col-user') }}
                  </th>
                  <th class="text-left px-3 py-2 font-medium">
                    {{ $t('admin-workspace-members-col-role') }}
                  </th>
                  <th class="text-left px-3 py-2 font-medium">
                    {{ $t('admin-workspace-members-col-invited') }}
                  </th>
                  <th class="text-left px-3 py-2 font-medium">
                    {{ $t('admin-workspace-members-col-accepted') }}
                  </th>
                  <th class="px-3 py-2"></th>
                </tr>
              </thead>
              <tbody class="divide-y divide-subtle">
                <tr v-for="member in members" :key="member.user_uuid" class="bg-surface">
                  <td class="px-3 py-2">
                    <div class="flex items-center gap-2.5 min-w-0">
                      <UserAvatar
                        :uuid="member.user_uuid"
                        :fallbackName="memberDisplayName(member.user_uuid)"
                        :showName="false"
                        size="xs"
                      />
                      <div class="min-w-0">
                        <div class="text-primary truncate">
                          {{ memberDisplayName(member.user_uuid) }}
                          <span
                            v-if="member.user_uuid === myUuid"
                            class="text-tertiary font-normal"
                          >
                            {{ $t('workspace-members-you') }}
                          </span>
                        </div>
                        <div class="text-xs text-tertiary font-mono truncate">
                          {{ member.user_uuid }}
                        </div>
                      </div>
                    </div>
                  </td>
                  <td class="px-3 py-2">
                    <BaseDropdown
                      v-if="canEditRow(member)"
                      :model-value="member.role"
                      :options="roleOptionsFor(member)"
                      size="sm"
                      :disabled="isSaving"
                      @update:model-value="onRoleChange(member, String($event))"
                    />
                    <span
                      v-else
                      class="inline-flex items-center px-2 py-1 rounded-md bg-surface-alt text-secondary text-xs"
                      :title="
                        isSoleOwner(member)
                          ? $t('admin-workspace-members-last-owner-hint')
                          : undefined
                      "
                    >
                      {{ roleLabel(member.role) }}
                    </span>
                  </td>
                  <td class="px-3 py-2 text-secondary whitespace-nowrap">
                    {{ formatWhen(member.invited_at) }}
                  </td>
                  <td class="px-3 py-2 text-secondary whitespace-nowrap">
                    {{ formatWhen(member.accepted_at) }}
                  </td>
                  <td class="px-3 py-2">
                    <button
                      v-if="canEditRow(member)"
                      type="button"
                      class="p-1.5 rounded-md transition-colors ml-auto block text-secondary hover:text-status-error hover:bg-status-error/10"
                      :aria-label="$t('admin-workspace-members-remove')"
                      :title="$t('admin-workspace-members-remove')"
                      :disabled="isSaving"
                      @click="requestRemove(member)"
                    >
                      <Icon name="trash" class="w-3.5 h-3.5" />
                    </button>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </template>
    </div>

    <ConfirmModal
      :show="memberToRemove !== null"
      variant="danger"
      :title="$t('admin-workspace-members-remove-confirm-title')"
      :message="
        memberToRemove
          ? t('admin-workspace-members-remove-confirm-message', {
              name: memberDisplayName(memberToRemove.user_uuid),
            })
          : ''
      "
      :confirm-label="$t('admin-workspace-members-remove-confirm-label')"
      :loading="isSaving"
      @confirm="confirmRemove"
      @close="memberToRemove = null"
    />
  </div>
</template>
