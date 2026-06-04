/**
 * Focus-managed inline rename, shared by the project row and card so
 * the edit affordance behaves identically in both. `start(current)`
 * flips to edit mode and focuses/selects the input; `done()` commits
 * the trimmed draft via the supplied callback; `cancel()` discards.
 * Bind `inputEl` as the input's template ref.
 */
import { nextTick, ref } from 'vue'

export function useInlineRename(commit: (name: string) => void) {
  const editing = ref(false)
  const draft = ref('')
  const inputEl = ref<HTMLInputElement | null>(null)

  async function start(current: string): Promise<void> {
    draft.value = current
    editing.value = true
    await nextTick()
    inputEl.value?.focus()
    inputEl.value?.select()
  }

  function done(): void {
    if (!editing.value) return
    editing.value = false
    commit(draft.value.trim())
  }

  function cancel(): void {
    editing.value = false
  }

  return { editing, draft, inputEl, start, done, cancel }
}
