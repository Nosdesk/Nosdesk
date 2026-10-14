<!--
PluginDialogHost: renders the dialogs plugins ask the host to draw
(`api.ui.confirm` and the `api.ui.pick*` calls; see usePluginDialogs.ts).

Mounted once at app root, after PluginModalHost. Every pending request renders
as its own dialog, in order, so a later one stacks over an earlier one and over
an open plugin modal (Reka layers: Escape and outside clicks reach only the top
one). Each reuses the app's own dialog for the job. Every dialog names the
plugin, in the title or under it, so a plugin can't pass itself off as the app.
-->
<script setup lang="ts">
import { ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import Modal from '@/components/Modal.vue';
import Button from '@/components/common/Button.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import DatePicker from '@/components/common/DatePicker.vue';
import UserSelectionModal from '@/components/UserSelectionModal.vue';
import AssetSelectionModal from '@/components/ticketComponents/AssetSelectionModal.vue';
import TicketPickerModal from '@/components/ticketComponents/TicketPickerModal.vue';
import type { Asset } from '@nosdesk/core/types/asset';
import { pluginDialogs, settlePluginDialog, type PluginDialogRequest } from '../usePluginDialogs';

const fluent = useFluent();
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args);

const PICK_TITLE_KEYS = {
  user: 'plugin-dialog-pick-user-title',
  asset: 'plugin-dialog-pick-asset-title',
  ticket: 'plugin-dialog-pick-ticket-title',
  date: 'plugin-dialog-pick-date-title',
} as const;

/** The plugin's own title when it gave one, else ours naming the plugin. */
function titleFor(req: PluginDialogRequest): string {
  if (req.options.title) return req.options.title;
  return req.kind === 'confirm' ? '' : t(PICK_TITLE_KEYS[req.kind], { plugin: req.pluginName });
}

/** The attribution subtitle, needed whenever the title is the plugin's own. */
function descriptionFor(req: PluginDialogRequest): string | undefined {
  return req.options.title ? t('plugin-dialog-attribution', { plugin: req.pluginName }) : undefined;
}

function onUser(id: number, user: { uuid: string; name: string }) {
  settlePluginDialog(id, user.uuid ? { uuid: user.uuid, name: user.name } : null);
}

function onAsset(id: number, asset: Asset) {
  settlePluginDialog(id, { id: asset.id, name: asset.name, assetTag: asset.asset_tag ?? null });
}

function onTicket(id: number, ticket: { id: number; title: string }) {
  settlePluginDialog(id, { id: ticket.id, title: ticket.title });
}

// The date each open date dialog holds, by request id, until the user chooses.
const dates = ref<Record<number, string>>({});
watch(
  pluginDialogs,
  (list) => {
    const next: Record<number, string> = {};
    for (const req of list) {
      if (req.kind === 'date') next[req.id] = dates.value[req.id] ?? req.options.initial ?? '';
    }
    dates.value = next;
  },
  { immediate: true },
);

function chooseDate(id: number) {
  const value = dates.value[id];
  if (value) settlePluginDialog(id, value);
}
</script>

<template>
  <template v-for="req in pluginDialogs" :key="req.id">
    <ConfirmModal
      v-if="req.kind === 'confirm'"
      show
      :title="req.options.title"
      :description="t('plugin-dialog-attribution', { plugin: req.pluginName })"
      :message="req.options.message ?? ''"
      :confirm-label="req.options.confirmLabel"
      :cancel-label="req.options.cancelLabel"
      :variant="req.options.danger ? 'danger' : 'info'"
      @confirm="settlePluginDialog(req.id, true)"
      @close="settlePluginDialog(req.id)"
    />

    <UserSelectionModal
      v-else-if="req.kind === 'user'"
      show
      scope="requester"
      :title="titleFor(req)"
      :description="descriptionFor(req)"
      @select-user="(u) => onUser(req.id, u)"
      @close="settlePluginDialog(req.id)"
    />

    <AssetSelectionModal
      v-else-if="req.kind === 'asset'"
      show
      hide-create
      :title="titleFor(req)"
      :description="descriptionFor(req)"
      @select-device="(a) => onAsset(req.id, a)"
      @close="settlePluginDialog(req.id)"
    />

    <TicketPickerModal
      v-else-if="req.kind === 'ticket'"
      show
      :title="titleFor(req)"
      :description="descriptionFor(req)"
      @select="(tk) => onTicket(req.id, tk)"
      @close="settlePluginDialog(req.id)"
    />

    <Modal
      v-else-if="req.kind === 'date'"
      show
      size="sm"
      :title="titleFor(req)"
      :description="descriptionFor(req)"
      @close="settlePluginDialog(req.id)"
    >
      <form class="flex flex-col gap-2" @submit.prevent="chooseDate(req.id)">
        <DatePicker
          v-model="dates[req.id]"
          block
          :label="t('plugin-dialog-date-label')"
        />
      </form>
      <template #footer>
        <div class="modal-actions">
          <Button variant="ghost" @click="settlePluginDialog(req.id)">
            {{ t('common-cancel') }}
          </Button>
          <Button variant="primary" :disabled="!dates[req.id]" @click="chooseDate(req.id)">
            {{ t('plugin-dialog-choose') }}
          </Button>
        </div>
      </template>
    </Modal>
  </template>
</template>
