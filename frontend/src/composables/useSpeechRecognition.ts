import { ref, readonly, computed } from 'vue'

// TypeScript declarations for Web Speech API
interface SpeechRecognitionEvent extends Event {
  results: SpeechRecognitionResultList
  resultIndex: number
}

interface SpeechRecognitionResultList {
  readonly length: number
  item(index: number): SpeechRecognitionResult
  [index: number]: SpeechRecognitionResult
}

interface SpeechRecognitionResult {
  readonly length: number
  item(index: number): SpeechRecognitionAlternative
  readonly isFinal: boolean
  [index: number]: SpeechRecognitionAlternative
}

interface SpeechRecognitionAlternative {
  readonly transcript: string
  readonly confidence: number
}

interface SpeechRecognition extends EventTarget {
  continuous: boolean
  interimResults: boolean
  lang: string
  start(): void
  stop(): void
  abort(): void
  onresult: ((event: SpeechRecognitionEvent) => void) | null
  onerror: ((event: Event) => void) | null
  onend: (() => void) | null
}

declare global {
  interface Window {
    SpeechRecognition: new () => SpeechRecognition
    webkitSpeechRecognition: new () => SpeechRecognition
  }
}

/**
 * Web Speech API composable for real-time transcription
 *
 * Features:
 * - Silent fallback when API unavailable
 * - Real-time interim results
 * - Auto-restart on speech pause (continuous mode)
 */
export function useSpeechRecognition() {
  const isSupported = ref(false)
  const isListening = ref(false)
  const transcript = ref('')
  const interimTranscript = ref('')

  let recognition: SpeechRecognition | null = null
  let shouldRestart = false

  // Check for Web Speech API support
  const SpeechRecognitionAPI = typeof window !== 'undefined'
    ? window.SpeechRecognition || window.webkitSpeechRecognition
    : null
  isSupported.value = !!SpeechRecognitionAPI
  console.log('[useSpeechRecognition] API supported:', isSupported.value)

  function start(): void {
    if (!SpeechRecognitionAPI) return

    recognition = new SpeechRecognitionAPI()
    recognition.continuous = true
    recognition.interimResults = true
    recognition.lang = 'en-US'

    shouldRestart = true

    recognition.onresult = (event: SpeechRecognitionEvent) => {
      let finalText = ''
      let interimText = ''

      for (let i = event.resultIndex; i < event.results.length; i++) {
        const result = event.results[i]
        if (result.isFinal) {
          finalText += result[0].transcript + ' '
        } else {
          interimText += result[0].transcript
        }
      }

      if (finalText) {
        transcript.value += finalText
      }
      interimTranscript.value = interimText
      console.log('[useSpeechRecognition] Result:', { final: finalText, interim: interimText })
    }

    recognition.onerror = (event: Event) => {
      console.log('[useSpeechRecognition] Error:', event)
      // Feature degrades gracefully
    }

    recognition.onend = () => {
      isListening.value = false
      // Auto-restart if still recording (handles speech pauses)
      if (shouldRestart && recognition) {
        try {
          recognition.start()
          isListening.value = true
        } catch {
          // Ignore restart errors
        }
      }
    }

    try {
      recognition.start()
      isListening.value = true
    } catch {
      // Silently fail - user can still record without transcription
    }
  }

  function stop(): void {
    shouldRestart = false
    if (recognition) {
      recognition.stop()
      recognition = null
    }
    isListening.value = false
  }

  function reset(): void {
    transcript.value = ''
    interimTranscript.value = ''
  }

  // Reactive full transcript (for template binding)
  const fullTranscript = computed(() => (transcript.value + interimTranscript.value).trim())

  return {
    isSupported: readonly(isSupported),
    isListening: readonly(isListening),
    transcript: readonly(transcript),
    interimTranscript: readonly(interimTranscript),
    fullTranscript,
    start,
    stop,
    reset
  }
}
