import { ref, type Ref } from 'vue';
import { useFluent } from 'fluent-vue';
import { extractErrorMessage } from '@/utils/errors';
import { useToastStore } from '@nosdesk/core/stores/toast';

/** Shared shape of a multi-valued contact row (phone, address). */
export interface ContactRow {
  id: number;
  is_primary: boolean;
  /** 'microsoft' etc. = directory-synced/read-only; null = manual. */
  source: string | null;
}

interface ContactApi<T, Input> {
  list: (uuid: string) => Promise<T[]>;
  add: (uuid: string, body: Input) => Promise<T>;
  update: (uuid: string, id: number, body: Input) => Promise<T>;
  remove: (uuid: string, id: number) => Promise<void>;
}

/**
 * The load/add/set-primary/delete lifecycle shared by UserPhonesCard and
 * UserAddressesCard. `toInput` maps a row back to its update body (so
 * set-primary re-sends the row with is_primary flipped). Errors surface as
 * toasts; `add` returns whether it succeeded so the caller can reset its draft.
 */
export function useContactList<T extends ContactRow, Input>(opts: {
  uuid: () => string;
  api: ContactApi<T, Input>;
  errorKeys: { load: string; save: string; delete: string };
  toInput: (row: T) => Input;
}) {
  const fluent = useFluent();
  const toast = useToastStore();
  const t = (key: string) => fluent.$t(key);

  const items = ref([]) as Ref<T[]>;
  const showAddForm = ref(false);
  const saving = ref(false);
  const pendingDelete = ref(null) as Ref<T | null>;

  async function load(): Promise<void> {
    try {
      items.value = await opts.api.list(opts.uuid());
    } catch (err) {
      toast.error(extractErrorMessage(err, t(opts.errorKeys.load)));
    }
  }

  async function add(body: Input): Promise<boolean> {
    saving.value = true;
    try {
      await opts.api.add(opts.uuid(), body);
      showAddForm.value = false;
      await load();
      return true;
    } catch (err) {
      toast.error(extractErrorMessage(err, t(opts.errorKeys.save)));
      return false;
    } finally {
      saving.value = false;
    }
  }

  async function setPrimary(row: T): Promise<void> {
    try {
      await opts.api.update(opts.uuid(), row.id, { ...opts.toInput(row), is_primary: true });
      await load();
    } catch (err) {
      toast.error(extractErrorMessage(err, t(opts.errorKeys.save)));
    }
  }

  async function doDelete(): Promise<void> {
    const target = pendingDelete.value;
    pendingDelete.value = null;
    if (!target) return;
    try {
      await opts.api.remove(opts.uuid(), target.id);
      await load();
    } catch (err) {
      toast.error(extractErrorMessage(err, t(opts.errorKeys.delete)));
    }
  }

  return { items, showAddForm, saving, pendingDelete, load, add, setPrimary, doDelete };
}
