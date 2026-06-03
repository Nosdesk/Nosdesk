/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Set to "true" to load Twemoji SVGs from CDN instead of bundled assets */
  readonly VITE_TWEMOJI_CDN?: string
  /** Git SHA stamped at build time, attached to bug reports so an
   *  admin can correlate the report with a specific bundle. Falls
   *  back to "dev" when the build arg is unset. */
  readonly VITE_BUILD_SHA?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
