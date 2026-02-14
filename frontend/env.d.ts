/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Set to "true" to load Twemoji SVGs from CDN instead of bundled assets */
  readonly VITE_TWEMOJI_CDN?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
