import { ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { useToastStore } from '@nosdesk/core/stores/toast'
import { saveFile, type FileSource } from '@/platform/files'

/**
 * Save a file (`saveFile`, `@/platform/files`) and report a failure with a
 * toast, for a caller with no error UI of its own. `saving` is true while a
 * save runs; in the app that covers the download and the share sheet.
 * Dismissing the sheet is not a failure.
 */
export function useFileSave() {
  const { $t } = useFluent()
  const saving = ref(false)

  /**
   * Resolves true once the file is saved (on the web, handed to the browser),
   * false when the user dismissed the share sheet or the save failed.
   */
  async function save(source: FileSource, filename?: string): Promise<boolean> {
    saving.value = true
    try {
      return (await saveFile(source, filename)).completed
    } catch (error) {
      console.error('Saving the file failed:', error)
      // The store is only needed on failure, so a page of links sets up none.
      useToastStore().error($t('common-file-save-failed'))
      return false
    } finally {
      saving.value = false
    }
  }

  return { save, saving }
}
