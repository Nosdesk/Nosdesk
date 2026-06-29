// Service for handling authenticated file access
import { logger } from '@nosdesk/core/utils/logger';
import { assetUrl } from '@nosdesk/core/transport';

// Generate an authenticated URL for a file
// Note: Authentication is handled via httpOnly cookies automatically by the browser
// This function just converts paths to use the authenticated API endpoints
export const getAuthenticatedFileUrl = (filePath: string): string => {
  // Just return the path as-is - cookies will be sent automatically
  return filePath
}

// Convert old upload paths to new authenticated API paths
export const convertToAuthenticatedPath = (originalPath: string): string => {
  // Normalise legacy /uploads paths to the authenticated /api/files endpoint.
  let path = originalPath
  if (originalPath.startsWith('/uploads/tickets/')) {
    path = `/api/files/tickets/${originalPath.replace('/uploads/tickets/', '')}`
  } else if (originalPath.startsWith('/uploads/temp/')) {
    path = `/api/files/temp/${originalPath.replace('/uploads/temp/', '')}`
  }

  // Identity on web (a relative path resolves to the app origin and the cookie
  // authenticates). On mobile this rewrites to the `nosdesk-asset` scheme so the
  // webview can load the file with auth (see core transport `assetUrl`).
  return assetUrl(path)
}

// Download a file with authentication
// Note: Authentication is handled via httpOnly cookies sent automatically
export const downloadAuthenticatedFile = async (filePath: string, filename?: string): Promise<void> => {
  try {
    // Fetch includes credentials (cookies) automatically with same-origin requests
    const response = await fetch(filePath, {
      credentials: 'same-origin'
    })

    if (!response.ok) {
      throw new Error(`Failed to download file: ${response.statusText}`)
    }

    const blob = await response.blob()
    const url = window.URL.createObjectURL(blob)
    const link = document.createElement('a')
    link.href = url
    link.download = filename || 'download'
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
    window.URL.revokeObjectURL(url)
  } catch (error) {
    logger.error('Error downloading file:', error)
    throw error
  }
}

export default {
  getAuthenticatedFileUrl,
  convertToAuthenticatedPath,
  downloadAuthenticatedFile
} 