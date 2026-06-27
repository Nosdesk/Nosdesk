/**
 * Optimistic bulk action with Undo toast.
 *
 * The 2025 Linear / Asana / Gmail pattern for reversible bulk
 * operations: apply the change immediately (UI updates instantly),
 * then surface an "Undone" / "Undo" toast so the user can roll back
 * if they didn't mean it. If the server rejects the change, the
 * `do` promise rejects and we surface an error toast (the caller is
 * responsible for whatever rollback the optimistic UI mutation
 * needs, since this helper doesn't know what was changed).
 *
 * Reserved for *reversible* operations. For irreversible ones use
 * `BulkConfirmDialog` instead and skip the optimism.
 *
 * Not a Vue composable — no `use*` prefix because it's a one-shot
 * imperative call. Callers pass already-localised `successTitle` and
 * `errorTitle` (typically the result of a `t('key', { count })`
 * lookup with a Fluent plural selector) so this helper never has to
 * pluralise English fragments itself.
 *
 *   await optimisticBulkAction({
 *     successTitle: t('tickets-bulk-archived', { count: ids.length }),
 *     errorTitle: t('tickets-bulk-archive-failed', { count: ids.length }),
 *     do: () => api.bulkArchive(ids),
 *     undo: () => api.bulkUnarchive(ids),
 *   })
 */
import { translate } from '@/i18n'
import { useToastStore } from '@nosdesk/core/stores/toast'

export interface OptimisticBulkActionOptions {
  /** Already-localised success toast title (e.g. "12 tickets archived").
   *  Build with `t('<key>', { count })` so plural rules apply. */
  successTitle: string
  /** Already-localised error toast title shown when `do` rejects. */
  errorTitle: string
  /** Run the optimistic operation (server call). Awaited so the
   *  helper can show an error toast if it rejects. */
  do: () => Promise<void>
  /** Run the rollback when the user clicks Undo. Awaited so any
   *  rollback failure surfaces as an error toast too. */
  undo: () => Promise<void>
}

export async function optimisticBulkAction(
  options: OptimisticBulkActionOptions,
): Promise<void> {
  const toast = useToastStore()
  const { successTitle, errorTitle } = options

  try {
    await options.do()
    toast.success(successTitle, undefined, {
      label: translate('bulk-action-undo', undefined, 'Undo'),
      handler: async () => {
        try {
          await options.undo()
          toast.info(translate('bulk-action-undone', undefined, 'Undone'))
        } catch (err) {
          toast.error(
            translate('bulk-action-undo-failed', undefined, 'Undo failed'),
            err instanceof Error ? err.message : undefined,
          )
        }
      },
    })
  } catch (err) {
    toast.error(errorTitle, err instanceof Error ? err.message : undefined)
    throw err
  }
}
