// Service for handling authenticated file access
import apiConfig from './apiConfig'

// Generate an authenticated URL for a file
// Note: Authentication is handled via httpOnly cookies automatically by the browser
// This function just converts paths to use the authenticated API endpoints
export const getAuthenticatedFileUrl = (filePath: string): string => {
  // Just return the path as-is - cookies will be sent automatically
  return filePath
}

// Convert old upload paths to new authenticated API paths
export const convertToAuthenticatedPath = (originalPath: string): string => {
  // Handle paths that already use /api/files
  if (originalPath.startsWith('/api/files/')) {
    return originalPath
  }

  // Convert /uploads/tickets/... to /api/files/tickets/...
  if (originalPath.startsWith('/uploads/tickets/')) {
    const filename = originalPath.replace('/uploads/tickets/', '')
    return `/api/files/tickets/${filename}`
  }

  // Convert /uploads/temp/... to /api/files/temp/...
  if (originalPath.startsWith('/uploads/temp/')) {
    const filename = originalPath.replace('/uploads/temp/', '')
    return `/api/files/temp/${filename}`
  }

  // For other paths (like user avatars), return as-is since they're public
  return originalPath
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
    console.error('Error downloading file:', error)
    throw error
  }
}

export default {
  getAuthenticatedFileUrl,
  convertToAuthenticatedPath,
  downloadAuthenticatedFile
} 