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
 * Reserved for *reversible* operations. For irreversible ones
 * (`actionVerb === 'delete'` of records that aren't soft-deleted,
 * sending an email, etc.) use `BulkConfirmDialog` instead and
 * skip the optimism.
 *
 * Not a Vue composable — no `use*` prefix because it's a one-shot
 * imperative call. Import and invoke from a click handler:
 *
 *   await optimisticBulkAction({
 *     count: selection.selectedCount.value,
 *     itemLabel: 'ticket',
 *     actionVerb: 'archived',
 *     do: () => api.bulkArchive(ids),
 *     undo: () => api.bulkUnarchive(ids),
 *   })
 */
import { translate } from '@/i18n'
import { useToastStore } from '@/stores/toast'

export interface OptimisticBulkActionOptions {
  /** Number of items the action applied to. Used in the toast copy. */
  count: number
  /** Singular item label, e.g. `"ticket"`. Pluralised for counts. */
  itemLabel: string
  /** Past-tense verb describing what just happened, e.g. `"archived"`,
   *  `"reassigned"`. Renders as `"12 tickets archived"`. */
  actionVerb: string
  /** Run the optimistic operation (server call). Awaited so the
   *  helper can show an error toast if it rejects. */
  do: () => Promise<void>
  /** Run the rollback when the user clicks Undo. Awaited so any
   *  rollback failure surfaces as an error toast too. */
  undo: () => Promise<void>
  /** Override the success message ("12 tickets archived"). */
  successTitle?: string
  /** Override the error title shown when `do` rejects. */
  errorTitle?: string
}

export async function optimisticBulkAction(
  options: OptimisticBulkActionOptions,
): Promise<void> {
  const toast = useToastStore()
  const plural = options.count === 1 ? options.itemLabel : `${options.itemLabel}s`
  const successTitle =
    options.successTitle ?? `${options.count} ${plural} ${options.actionVerb}`
  const errorTitle =
    options.errorTitle ?? `Failed to ${options.actionVerb.replace(/ed$/, '')} ${plural}`

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
