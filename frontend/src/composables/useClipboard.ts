import { ref, onScopeDispose } from 'vue'

export function useClipboard() {
  const copied = ref(false)
  let resetTimeout: ReturnType<typeof setTimeout> | null = null

  function clearReset() {
    if (resetTimeout) {
      clearTimeout(resetTimeout)
      resetTimeout = null
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text)
    } catch {
      const textArea = document.createElement('textarea')
      textArea.value = text
      document.body.appendChild(textArea)
      textArea.select()
      document.execCommand('copy')
      document.body.removeChild(textArea)
    }
    clearReset()
    copied.value = true
    resetTimeout = setTimeout(() => { copied.value = false }, 2000)
  }

  onScopeDispose(clearReset)

  return { copied, copy }
}
