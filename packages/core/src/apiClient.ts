/**
 * The shared axios instance.
 *
 * Headless on purpose: this module only creates the instance so that core
 * services (which import `apiClient`) carry no host dependency. All behaviour,
 * base URL and credential mode (from the transport seam), auth headers,
 * correlation/diagnostics headers, logging, error typing, and the 401
 * refresh/retry state machine, is registered as interceptors by the host at
 * bootstrap (web: frontend/src/services/apiConfig.ts). Each surface wires its
 * own; the Tauri app registers a bearer-mode equivalent.
 */
import axios from 'axios'

const apiClient = axios.create({
  headers: {
    'Content-Type': 'application/json',
  },
})

export default apiClient
