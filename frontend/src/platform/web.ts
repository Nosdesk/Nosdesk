/**
 * The web platform's core seams. Each module configures one seam on import,
 * and static imports run in the order written. One module, so the browser
 * fetches them together rather than one after another, a round trip each.
 */
import '@/utils/loggerSetup'
import '@/utils/storageSetup'
import '@/services/transport'
import '@/services/apiConfig'
