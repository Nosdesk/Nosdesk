<script setup lang="ts">
/**
 * Per-workspace member management (Phase 4 W3).
 *
 * Invite users via the shared UserPicker (requester scope), change
 * roles, and remove members. Surfaces backend 409 `last_owner` with
 * a clear message; disables demote/remove on the sole owner in the UI.
 */
import { computed, ref } from 'vue';
import { useRoute } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import { formatDistanceToNow } from 'date-fns';

import AlertMessage from '@/components/common/AlertMessage.vue';
import BackButton from '@/components/common/BackButton.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Button from '@/components/common/Button.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Icon from '@/components/common/Icon.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import UserPicker from '@/components/ticketComponents/UserPicker.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import workspacesService from '@/services/workspacesService';
import type { WorkspaceMember, WorkspaceRole } from '@/types/workspace';
import { extractErrorMessage } from '@/utils/errors';
import * as syncPool from '@/sync/pool';
import type { User } from '@/types/user';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const WORKSPACE_ROLES: WorkspaceRole[] = ['owner', 'admin', 'agent', 'member'];

const route = useRoute();
const workspaceId = computed(() => Number(route.params.id));

const MEMBERS_KEY = computed(
  () => ['admin-workspace-members', workspaceId.value] as const,
);
const WORKSPACES_LIST_KEY = ['admin-workspaces', true] as const;

const queryCache = useQueryCache();

const membersQuery = useQuery({
  key: MEMBERS_KEY,
  query: () => workspacesService.listMembers(workspaceId.value),
});
const workspacesQuery = useQuery({
  key: WORKSPACES_LIST_KEY,
  query: () => workspacesService.list(true),
});

const members = computed<WorkspaceMember[]>(() =>
  Array.isArray(membersQuery.data.value) ? membersQuery.data.value : [],
);
const workspace = computed(() =>
  (Array.isArray(workspacesQuery.data.value)
    ? workspacesQuery.data.value
    : []
  ).find((w) => w.id === workspaceId.value),
);

const isFirstLoad = computed(
  () =>
    membersQuery.status.value === 'pending' &&
    membersQuery.data.value === undefined,
);
const loadError = computed(() =>
  membersQuery.error.value ? t('admin-workspace-members-error-load') : '',
);

const ownerCount = computed(
  () => members.value.filter((m) => m.role === 'owner').length,
);
const memberUuidSet = computed(
  () => new Set(members.value.map((m) => m.user_uuid)),
);

const isSaving = ref(false);
const errorMessage = ref('');
const successMessage = ref('');

const inviteUserUuid = ref('');
const inviteRole = ref<WorkspaceRole>('member');
const memberToRemove = ref<WorkspaceMember | null>(null);

const refresh = () => queryCache.invalidateQueries({ key: MEMBERS_KEY.value });

function flashSuccess(message: string) {
  successMessage.value = message;
  setTimeout(() => {
    successMessage.value = '';
  }, 3000);
}

function isLastOwnerError(error: unknown): boolean {
  const e = error as { response?: { status?: number; data?: { error?: string } } };
  return e.response?.status === 409 && e.response?.data?.error === 'last_owner';
}

function resolveMutationError(error: unknown, fallbackKey: string): string {
  if (isLastOwnerError(error)) {
    return t('admin-workspace-members-error-last-owner');
  }
  return extractErrorMessage(error, t(fallbackKey));
}

function isSoleOwner(member: WorkspaceMember): boolean {
  return member.role === 'owner' && ownerCount.value === 1;
}

function memberDisplayName(uuid: string): string {
  const cached = syncPool.get('user', uuid) as Pick<User, 'name' | 'email'> | undefined;
  return cached?.name ?? cached?.email ?? uuid;
}

function roleLabel(role: WorkspaceRole): string {
  return t(`admin-workspace-members-role-${role}`);
}

const inviteRoleOptions = computed(() =>
  WORKSPACE_ROLES.map((role) => ({ value: role, label: roleLabel(role) })),
);

/** Per-row role options. The sole owner can't be demoted, so every
 *  non-owner option is disabled in that row (the whole control is also
 *  disabled, but this keeps the menu honest). */
function roleOptionsFor(member: WorkspaceMember) {
  const sole = isSoleOwner(member);
  return WORKSPACE_ROLES.map((role) => ({
    value: role,
    label: roleLabel(role),
    disabled: sole && role !== 'owner',
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

async function addMember(userUuid: string, role: WorkspaceRole) {
  isSaving.value = true;
  errorMessage.value = '';
  try {
    const result = await workspacesService.addMember(workspaceId.value, {
      user_uuid: userUuid,
      role,
    });
    if ('status' in result && result.status === 'already_member') {
      flashSuccess(t('admin-workspace-members-already-member'));
    } else {
      flashSuccess(
        t('admin-workspace-members-added-success', {
          name: memberDisplayName(userUuid),
        }),
      );
    }
    inviteUserUuid.value = '';
    refresh();
  } catch (e: unknown) {
    errorMessage.value = resolveMutationError(e, 'admin-workspace-members-error-add');
  } finally {
    isSaving.value = false;
  }
}

async function changeRole(userUuid: string, role: WorkspaceRole) {
  isSaving.value = true;
  errorMessage.value = '';
  try {
    await workspacesService.updateMemberRole(workspaceId.value, userUuid, { role });
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
    await workspacesService.removeMember(workspaceId.value, userUuid);
    memberToRemove.value = null;
    refresh();
  } catch (e: unknown) {
    errorMessage.value = resolveMutationError(e, 'admin-workspace-members-error-remove');
  } finally {
    isSaving.value = false;
  }
}

function submitInvite() {
  if (workspace.value?.archived_at) {
    errorMessage.value = t('admin-workspace-members-error-archived');
    return;
  }
  const uuid = inviteUserUuid.value.trim();
  if (!uuid) {
    errorMessage.value = t('admin-workspace-members-error-user-required');
    return;
  }
  if (memberUuidSet.value.has(uuid)) {
    errorMessage.value = t('admin-workspace-members-already-member');
    return;
  }
  void addMember(uuid, inviteRole.value);
}

function onRoleChange(member: WorkspaceMember, value: string) {
  const newRole = value as WorkspaceRole;
  if (newRole === member.role || isSoleOwner(member)) return;
  void changeRole(member.user_uuid, newRole);
}

function requestRemove(member: WorkspaceMember) {
  if (isSoleOwner(member)) return;
  memberToRemove.value = member;
}

function confirmRemove() {
  const member = memberToRemove.value;
  if (!member) return;
  void removeMember(member.user_uuid);
}
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <header class="flex flex-col gap-3">
        <BackButton
          fallback-route="/admin/workspaces"
          :label="$t('admin-workspace-members-back')"
          compact
        />
        <div>
          <h1 class="text-xl sm:text-2xl font-bold text-primary">
            {{ $t('admin-workspace-members-title') }}
          </h1>
          <p v-if="workspace" class="text-secondary text-sm mt-1">
            {{
              t('admin-workspace-members-workspace-label', {
                name: workspace.name,
                slug: workspace.slug,
              })
            }}
          </p>
          <p v-else class="text-secondary text-sm mt-1">
            {{ t('admin-workspace-members-workspace-fallback', { id: workspaceId }) }}
          </p>
        </div>
      </header>

      <div
        v-if="workspace?.archived_at"
        class="bg-status-warning/10 border border-status-warning/30 rounded-lg p-3 text-sm text-status-warning flex items-start gap-2"
      >
        <Icon name="info" size="md" class="flex-shrink-0" />
        <span>{{ $t('admin-workspace-members-archived-notice') }}</span>
      </div>

      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />
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
        <!-- Invite -->
        <section
          class="bg-surface border border-default rounded-xl p-4 flex flex-col gap-4"
          :class="{ 'opacity-60 pointer-events-none': workspace?.archived_at }"
        >
          <h2 class="text-sm font-medium text-primary">
            {{ $t('admin-workspace-members-invite-heading') }}
          </h2>
          <div class="grid gap-4 sm:grid-cols-[1fr_auto_auto] sm:items-end">
            <div class="min-w-0">
              <label class="block text-xs font-medium text-secondary mb-1">
                {{ $t('admin-workspace-members-invite-user-label') }}
              </label>
              <div
                class="border border-default rounded-lg bg-surface-alt overflow-hidden"
              >
                <UserPicker
                  v-model="inviteUserUuid"
                  type="requester"
                  :placeholder="$t('admin-workspace-members-invite-user-placeholder')"
                />
              </div>
            </div>
            <div class="sm:w-40">
              <BaseDropdown
                :model-value="inviteRole"
                :options="inviteRoleOptions"
                :label="$t('admin-workspace-members-invite-role-label')"
                size="sm"
                :disabled="isSaving"
                @update:model-value="inviteRole = String($event) as WorkspaceRole"
              />
            </div>
            <Button
              class="sm:self-end shrink-0"
              icon="add"
              :loading="isSaving"
              :disabled="!!workspace?.archived_at"
              @click="submitInvite"
            >
              {{ $t('admin-workspace-members-invite-submit') }}
            </Button>
          </div>
        </section>

        <EmptyState
          v-if="members.length === 0"
          icon="users"
          :title="$t('empty-workspace-members-title')"
          :description="$t('empty-workspace-members-description')"
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
                        </div>
                        <div class="text-xs text-tertiary font-mono truncate">
                          {{ member.user_uuid }}
                        </div>
                      </div>
                    </div>
                  </td>
                  <td class="px-3 py-2">
                    <BaseDropdown
                      :model-value="member.role"
                      :options="roleOptionsFor(member)"
                      size="sm"
                      :disabled="isSaving || isSoleOwner(member)"
                      @update:model-value="onRoleChange(member, String($event))"
                    />
                  </td>
                  <td class="px-3 py-2 text-secondary whitespace-nowrap">
                    {{ formatWhen(member.invited_at) }}
                  </td>
                  <td class="px-3 py-2 text-secondary whitespace-nowrap">
                    {{ formatWhen(member.accepted_at) }}
                  </td>
                  <td class="px-3 py-2">
                    <button
                      type="button"
                      class="p-1.5 rounded-md transition-colors ml-auto block"
                      :class="
                        isSoleOwner(member)
                          ? 'text-tertiary cursor-not-allowed'
                          : 'text-secondary hover:text-status-error hover:bg-status-error/10'
                      "
                      :aria-label="$t('admin-workspace-members-remove')"
                      :title="
                        isSoleOwner(member)
                          ? $t('admin-workspace-members-last-owner-hint')
                          : $t('admin-workspace-members-remove')
                      "
                      :disabled="isSaving || isSoleOwner(member)"
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
