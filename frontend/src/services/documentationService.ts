import { logger } from '@/utils/logger';
import apiClient from './apiConfig';
import type { UserInfo } from '@/types/user';

// Note: apiClient already has baseURL set to '/api', so routes need no prefix

// Re-export for backwards compatibility
export type { UserInfo };

// Define the Page interface
export interface Page {
  id: string | number;
  uuid?: string;
  slug: string;
  title: string;
  description: string | null;
  content: string;
  parent_id: string | number | null;
  author: string; // Deprecated - use created_by.name instead
  status: string;
  icon: string | null;
  children: Page[];
  lastUpdated?: string;
  ticket_id?: string | null;
  display_order?: number;
  created_at?: string;
  updated_at?: string;
  created_by?: UserInfo;
  last_edited_by?: UserInfo;
  deleted_at?: string | null;
  archived_at?: string | null;
}

// Define the PageChild interface (used for navigation)
export interface PageChild {
  id: string | number;
  slug: string;
  title: string;
  description: string | null;
  parent_id: string | number | null;
  icon: string | null;
  path?: string;
  children?: PageChild[];
}

// For backward compatibility
export interface Article extends Omit<Page, 'children'> {
  children?: Page[];
}

// Backend interfaces
interface BackendDocumentationPage {
  id: number;
  slug: string;
  title: string;
  description: string | null;
  content: string;
  category_id: number | null;
  parent_id: number | null;
  author: string;
  status: 'draft' | 'published' | 'archived';
  icon: string | null;
  created_at: string;
  updated_at: string;
  children?: BackendDocumentationPage[];
}

function createFallbackPage(idPrefix: string, titlePrefix: string, icon = '❓'): Page {
  return {
    id: `${idPrefix}-${Date.now()}`,
    slug: idPrefix,
    title: `${titlePrefix} Page`,
    description: null,
    content: '',
    parent_id: null,
    author: 'System',
    status: 'draft',
    icon,
    children: [],
    lastUpdated: new Date().toISOString(),
  };
}

// Convert backend page data to frontend Page format
export const convertToPage = (data: unknown): Page => {
  // Handle null or undefined data
  if (!data || typeof data !== 'object') {
    logger.warn('Attempting to convert null or undefined data to Page');
    return createFallbackPage('invalid-page', 'Invalid');
  }

  // Type assertion after validation
  const pageData = data as Record<string, unknown>;

  try {
    // Clean up all properties before assigning
    const cleanId = pageData.id !== undefined ? pageData.id : `unknown-${Date.now()}`;
    const cleanSlug = typeof pageData.slug === 'string' ? pageData.slug : '';
    const cleanTitle = typeof pageData.title === 'string' ? pageData.title : 'Untitled';
    const cleanDescription = pageData.description !== undefined ? pageData.description : null;

    // Handle content conversion from binary (Vec<u8>) to string
    let cleanContent = '';
    if (pageData.content) {
      if (typeof pageData.content === 'string') {
        // Already a string
        cleanContent = pageData.content;
      } else if (Array.isArray(pageData.content)) {
        // It's an array of bytes from the backend
        try {
          const bytes = new Uint8Array(pageData.content as number[]);
          cleanContent = new TextDecoder().decode(bytes);
        } catch (decodeError) {
          logger.warn('Error decoding binary content:', decodeError);
          cleanContent = '';
        }
      } else {
        cleanContent = '';
      }
    }

    const cleanParentId = pageData.parent_id !== undefined ? pageData.parent_id : null;
    // For backwards compatibility, fallback to created_by.name if author doesn't exist
    const createdBy = pageData.created_by as { name?: string } | undefined;
    const cleanAuthor = typeof pageData.author === 'string' ? pageData.author : (createdBy?.name || 'System');
    const cleanStatus = typeof pageData.status === 'string' ? pageData.status : 'published';
    const cleanIcon = typeof pageData.icon === 'string' ? pageData.icon : '📄';

    // Process children array with extra validation
    let cleanChildren: Page[] = [];
    if (Array.isArray(pageData.children)) {
      cleanChildren = (pageData.children as unknown[])
        .filter((child) => child && typeof child === 'object') // Filter out non-objects
        .map((child) => convertToPage(child));                  // Convert each valid child
    }

    return {
      id: cleanId,
      uuid: pageData.uuid as string | undefined,
      slug: cleanSlug,
      title: cleanTitle,
      description: cleanDescription as string | null,
      content: cleanContent,
      parent_id: cleanParentId as string | number | null,
      author: cleanAuthor,
      status: cleanStatus,
      icon: cleanIcon as string | null,
      children: cleanChildren,
      lastUpdated: (pageData.updated_at as string) || new Date().toISOString(),
      ticket_id: (pageData.ticket_id as string | null) || null,
      created_at: pageData.created_at as string | undefined,
      updated_at: pageData.updated_at as string | undefined,
      created_by: pageData.created_by as UserInfo | undefined,
      last_edited_by: pageData.last_edited_by as UserInfo | undefined,
      display_order: typeof pageData.display_order === 'number' ? pageData.display_order : 0,
      deleted_at: (pageData.deleted_at as string | null) || null,
      archived_at: (pageData.archived_at as string | null) || null,
    };
  } catch (error) {
    logger.error('Error converting backend page data:', error, data);
    return createFallbackPage('error-page', 'Error', '⚠️');
  }
};

// Convert backend page data to frontend Article format (for backward compatibility)
export const convertToArticle = (data: unknown): Article => {
  const page = convertToPage(data);
  return { ...page, children: page.children };
};

/**
 * Get all top-level pages with their children
 */
export const getPages = async (): Promise<Page[]> => {
  try {
    // Fetch all pages
    const response = await apiClient.get(`/documentation/pages`);
    logger.debug('Raw API response for pages:', response.data);
    
    // Validate that the response is an array
    if (!Array.isArray(response.data)) {
      logger.error('API response is not an array:', response.data);
      return [];
    }
    
    // Filter out any potentially invalid items before conversion
    const validItems = (response.data as unknown[]).filter((item): item is Record<string, unknown> =>
      item !== null && typeof item === 'object' && 'id' in item
    );
    
    if (validItems.length !== response.data.length) {
      logger.warn(`Filtered out ${response.data.length - validItems.length} invalid items from API response`);
    }
    
    // Convert all pages to our frontend format
    const allPages = validItems.map(convertToArticle);
    
    // Map to store pages by ID for easy lookup
    const pagesMap = new Map<string | number, Page>();
    
    // First pass: Create a map of all pages by ID
    allPages.forEach((page: Article) => {
      pagesMap.set(page.id, {
        ...page,
        children: [] // Initialize empty children array
      });
    });
    
    // Second pass: Organize pages into parent-child hierarchy
    const topLevelPages: Page[] = [];
    
    allPages.forEach((page: Article) => {
      const pageWithChildren = pagesMap.get(page.id);
      
      if (!pageWithChildren) {
        logger.warn(`Page with ID ${page.id} not found in pagesMap`);
        return; // Skip this iteration
      }
      
      if (!page.parent_id) {
        // This is a top-level page
        topLevelPages.push(pageWithChildren);
      } else {
        // This is a child page, add it to its parent's children array if parent exists
        const parentPage = pagesMap.get(page.parent_id);
        if (parentPage) {
          if (!parentPage.children) {
            logger.warn(`Parent page ${page.parent_id} has no children array`);
            parentPage.children = []; // Create the children array if it doesn't exist
          }
          parentPage.children.push(pageWithChildren);
        } else {
          // If parent doesn't exist (orphaned child), add to top level
          logger.warn(`Page ${page.id} has parent_id ${page.parent_id} but parent not found, adding to top level`);
          topLevelPages.push(pageWithChildren);
        }
      }
    });
    
    // Sort children recursively by display_order
    const sortChildrenRecursively = (page: Page) => {
      if (page.children && page.children.length > 0) {
        page.children.sort((a, b) => {
          const orderA = a.display_order !== undefined && a.display_order !== null ? Number(a.display_order) : 999;
          const orderB = b.display_order !== undefined && b.display_order !== null ? Number(b.display_order) : 999;
          return orderA - orderB;
        });
        
        // Recursively sort grandchildren
        page.children.forEach(sortChildrenRecursively);
      }
    };
    
    // Apply recursive sorting
    topLevelPages.forEach(sortChildrenRecursively);
    
    // Print out the pages hierarchy to help debug
    logger.debug('Pages with proper hierarchy:');
    topLevelPages.forEach((page, index) => {
      logger.debug(`Top level page ${index + 1}: ${page.title} (ID: ${page.id})`);
      if (page.children && page.children.length > 0) {
        logger.debug(`  Has ${page.children.length} children`);
        page.children.forEach((child, childIndex) => {
          logger.debug(`    Child ${childIndex + 1}: ${child.title} (ID: ${child.id})`);
        });
      } else {
        logger.debug(`  No children`);
      }
    });
    
    logger.debug('Organized pages hierarchy:', JSON.stringify(topLevelPages, null, 2));
    return topLevelPages;
  } catch (error) {
    logger.error('Error fetching documentation pages:', error);
    return [];
  }
};

/**
 * Get all articles with metadata (no content)
 */
export const getAllArticles = async (): Promise<Article[]> => {
  try {
    // Fetch all documentation pages
    const response = await apiClient.get(`/documentation/pages`);
    
    // Convert backend pages to frontend Articles (without content)
    return response.data.map((page: BackendDocumentationPage) => {
      const { content, ...metadata } = convertToArticle(page);
      return metadata;
    });
  } catch (error) {
    logger.error('Error fetching documentation pages:', error);
    return [];
  }
};

/**
 * Get article by ID (slug or numeric ID)
 */
export const getArticleById = async (id: string | number): Promise<Article | null> => {
  try {
    let response;
    
    // If it's a numeric ID, use the direct ID endpoint
    if (!isNaN(Number(id))) {
      response = await apiClient.get(`/documentation/pages/${id}`);
    } else {
      // Otherwise use the slug endpoint
      response = await apiClient.get(`/documentation/pages/slug/${id}`);
    }
    
    // Convert backend page to frontend Article
    return convertToArticle(response.data);
  } catch (error) {
    logger.error(`Error fetching documentation page with ID ${id}:`, error);
    return null;
  }
};

/**
 * Get page by ID (slug) with its children
 */
export const getPageById = async (id: string): Promise<Page | null> => {
  try {
    // Fetch the page with its children
    const response = await apiClient.get(`/documentation/pages/slug/${id}/with-children`);
    
    // Convert backend page to frontend Page
    return convertToPage(response.data);
  } catch (error) {
    logger.error(`Error fetching documentation page with ID ${id}:`, error);
    return null;
  }
};

/**
 * Get a page by its path (slug or ID)
 */
export const getPageByPath = async (path: string): Promise<Page | null> => {
  try {
    // Handle empty path
    if (!path) {
      logger.error('Empty path provided to getPageByPath');
      return null;
    }

    // Check if the path is a numeric ID
    if (!isNaN(Number(path))) {
      try {
        logger.debug(`Fetching page with numeric ID: ${path}`);
        const response = await apiClient.get(`/documentation/pages/${path}`);
        return convertToPage(response.data);
      } catch (idError) {
        logger.error(`Error fetching page with ID ${path}:`, idError);
        return null;
      }
    } 
    // Otherwise, treat it as a slug
    else {
      try {
        logger.debug(`Fetching page with slug: ${path}`);
        const response = await apiClient.get(`/documentation/pages/slug/${path}`);
        return convertToPage(response.data);
      } catch (slugError) {
        logger.error(`Error fetching page with slug ${path}:`, slugError);
        return null;
      }
    }
  } catch (error) {
    logger.error(`Error in getPageByPath for ${path}:`, error);
    return null;
  }
};

/**
 * Search articles by query
 */
export const searchArticles = async (query: string): Promise<Article[]> => {
  try {
    // Try to use the backend search endpoint
    try {
      const response = await apiClient.get(`/documentation/search?q=${encodeURIComponent(query)}`);
      return (response.data as unknown[]).map((item) => convertToArticle(item));
    } catch (error) {
      logger.error('Backend search failed, falling back to client-side search:', error);
      
      // Fallback to client-side search
      const allArticlesResponse = await apiClient.get(`/documentation/pages`);
      const allArticles = (allArticlesResponse.data as unknown[]).map((item) => convertToArticle(item));
      
      // Filter articles by title and description
      const lowerQuery = query.toLowerCase();
      return allArticles.filter((article: Article) => 
        article.title.toLowerCase().includes(lowerQuery) ||
        (article.description && article.description.toLowerCase().includes(lowerQuery))
      );
    }
  } catch (searchError) {
    logger.error('Error with fallback search:', searchError);
    return [];
  }
};

/**
 * Save an article (update an existing article)
 */
/**
 * @deprecated This method is deprecated. Content is now automatically synced via WebSocket collaboration.
 * Only use this for initial creation or metadata updates. For content edits, use CollaborativeEditor.
 */
export const saveArticle = async (article: Page): Promise<Page | null> => {
  logger.warn('saveArticle is deprecated - content should be synced via WebSocket collaboration');
  try {
    // Determine if article ID is numeric or a slug
    let numericId: number;
    
    if (typeof article.id === 'number') {
      numericId = article.id;
    } else if (!isNaN(Number(article.id))) {
      numericId = Number(article.id);
    } else {
      // If it's a slug, fetch the numeric ID first
      try {
        const response = await apiClient.get(`/documentation/pages/slug/${article.id}`);
        numericId = response.data.id;
      } catch (error) {
        logger.error(`Error fetching article with slug ${article.id}:`, error);
        return null;
      }
    }
    
    // Fetch the current article to get its created_at and updated_at fields
    const currentArticleResponse = await apiClient.get(`/documentation/pages/${numericId}`);
    const currentArticle = currentArticleResponse.data;
    
    // Convert status string to enum value expected by backend
    let statusValue;
    if (typeof article.status === 'string') {
      statusValue = article.status.toLowerCase();
    } else {
      // Default to published
      statusValue = 'published';
    }
    
    // Convert content string to bytes for the backend
    const contentBytes = Array.from(new TextEncoder().encode(article.content));
    
    // Create a clean payload object with only the required fields
    // Important: Keep created_at and updated_at exactly as they are in the original article
    // The backend expects NaiveDateTime objects, not ISO strings
    const payload = {
      slug: article.slug,
      title: article.title,
      description: article.description || null,
      content: contentBytes, // Send as array of bytes
      parent_id: article.parent_id,
      status: statusValue,
      icon: article.icon,
      created_at: currentArticle.created_at,
      updated_at: currentArticle.updated_at
    };
    
    // Log the payload as a JSON string to check for any issues
    logger.debug('Saving article with payload:', JSON.stringify(payload));
    
    // Update the article
    const response = await apiClient.put(`/documentation/pages/${numericId}`, payload);
    
    // Fetch the updated article
    const updatedArticleResponse = await apiClient.get(`/documentation/pages/${numericId}`);
    
    // Convert backend article to frontend Page
    return convertToPage(updatedArticleResponse.data);
  } catch (error) {
    logger.error('Error saving article:', error);

    // Try to log more detailed error information
    const axiosError = error as { response?: { data?: unknown; status?: number }; config?: { data?: unknown } };
    if (axiosError.response) {
      logger.error('Update error response data:', axiosError.response.data);
      logger.error('Update error response status:', axiosError.response.status);

      // Try to log the request payload that caused the error
      if (axiosError.config?.data) {
        logger.error('Request payload that caused error:', axiosError.config.data);
      }
    }

    return null;
  }
};

/**
 * Create a new article
 */
export const createArticle = async (article: Partial<Page>): Promise<Page | null> => {
  try {
    // Convert status string to enum value expected by backend
    let statusValue;
    if (typeof article.status === 'string') {
      statusValue = article.status.toLowerCase();
    } else {
      // Default to draft
      statusValue = 'draft';
    }

    // Prepare the payload matching CreateDocumentationPageRequest.
    // Backend generates UUID, slug, and sets created_by /
    // last_edited_by from auth context. `collection_id` (when
    // provided) tells the backend to insert the new page into
    // that collection's junction table directly; when omitted,
    // the backend cascades from `parent_id` (so creating a
    // child of an existing page inherits the parent's collection
    // automatically).
    const collectionId = (article as Partial<Page> & { collection_id?: number }).collection_id;
    const payload = {
      title: article.title || 'Untitled',
      icon: article.icon || '📄',
      cover_image: null,
      status: statusValue,
      parent_id: article.parent_id !== undefined ? article.parent_id : null,
      ticket_id: article.ticket_id || null,
      display_order: article.display_order !== undefined ? article.display_order : 0,
      is_public: false,
      is_template: false,
      yjs_state_vector: null,
      yjs_document: null,
      yjs_client_id: null,
      has_unsaved_changes: false,
      ...(collectionId !== undefined ? { collection_id: collectionId } : {}),
    };

    // Print payload as a formatted string for debugging
    logger.debug('Creating article with payload:', JSON.stringify(payload, null, 2));

    // Create the article
    const response = await apiClient.post(`/documentation/pages`, payload);
    
    logger.debug('Article created successfully:', response.data);
    
    // Check if the response contains the created article
    if (!response.data || !response.data.id) {
      logger.error('Invalid response data from creating article:', response.data);
      return null;
    }
    
    try {
      // Fetch the created article using the correct endpoint
      const createdArticleResponse = await apiClient.get(`/documentation/pages/${response.data.id}`);
      
      // Convert backend article to frontend Page
      return convertToPage(createdArticleResponse.data);
    } catch (fetchError) {
      logger.warn('Error fetching the newly created article, returning original response:', fetchError);
      // If fetching the new article fails, return the data from the creation response
      return convertToPage(response.data);
    }
  } catch (error) {
    logger.error('Error creating article:', error);
    const axiosError = error as {
      response?: { data?: unknown; status?: number; headers?: unknown };
      config?: { url?: string; method?: string; data?: unknown };
      request?: unknown;
      message?: string;
    };
    if (axiosError.response) {
      logger.error('Response data:', axiosError.response.data);
      logger.error('Response status:', axiosError.response.status);
      logger.error('Response headers:', axiosError.response.headers);

      // Log more details about the request
      if (axiosError.config) {
        logger.error('Request URL:', axiosError.config.url);
        logger.error('Request method:', axiosError.config.method);
        logger.error('Request data:', axiosError.config.data);
      }
    } else if (axiosError.request) {
      logger.error('Request made but no response received:', axiosError.request);
    } else {
      logger.error('Error setting up request:', axiosError.message);
    }
    return null;
  }
};

/**
 * Get pages by parent ID
 */
export const getPagesByParentId = async (parentId: string): Promise<Page[]> => {
  try {
    // Fetch pages by parent ID
    const response = await apiClient.get(`/documentation/pages/parent/${parentId}`);
    
    // Convert backend pages to frontend Pages
    return response.data.map(convertToPage);
  } catch (error) {
    logger.error(`Error fetching pages with parent ID ${parentId}:`, error);
    return [];
  }
};

/**
 * Get page with its children by parent ID
 */
export const getPageWithChildrenByParentId = async (pageId: string): Promise<Page | null> => {
  try {
    // Fetch the page with its children
    const response = await apiClient.get(`/documentation/pages/${pageId}/with-children-by-parent`);
    
    // Convert backend page to frontend Page
    return convertToPage(response.data);
  } catch (error) {
    logger.error(`Error fetching page with children for ID ${pageId}:`, error);
    return null;
  }
};

/**
 * Update the parent of a documentation page
 * @param pageId The ID of the page to update
 * @param newParentId The new parent ID (null for top-level pages)
 */
export const updateParent = async (pageId: string, newParentId: string | number | null): Promise<Page | null> => {
  try {
    // Get the current page data
    const pageResponse = await apiClient.get(`/documentation/pages/${pageId}`);
    const page = pageResponse.data;
    
    // Create a copy of the page to modify
    const updatedPage = { ...page };
    
    // Update the parent_id - convert to number for backend
    if (newParentId === null) {
      updatedPage.parent_id = null;
    } else {
      // Convert to number since backend expects i32
      updatedPage.parent_id = typeof newParentId === 'string' ? parseInt(newParentId, 10) : newParentId;
    }
    
    // Save the updated article
    const updatedArticle = await saveArticle({
      id: updatedPage.id,
      slug: updatedPage.slug,
      title: updatedPage.title,
      description: updatedPage.description,
      content: updatedPage.content,
      parent_id: updatedPage.parent_id,
      author: updatedPage.author,
      status: updatedPage.status,
      icon: updatedPage.icon,
      children: [],
      lastUpdated: updatedPage.updated_at,
      ticket_id: updatedPage.ticket_id
    });
    
    return updatedArticle;
  } catch (error) {
    logger.error(`Error updating parent for page ${pageId}:`, error);
    return null;
  }
};

/**
 * Reorder pages under a parent
 */
export const reorderPages = async (parentId: string | number | null, pageOrders: { page_id: number, display_order: number }[]): Promise<boolean> => {
  try {
    await apiClient.post(`/documentation/pages/reorder`, {
      parent_id: parentId !== null ? Number(parentId) : null,
      page_orders: pageOrders,
    });
    return true;
  } catch (error) {
    logger.error('Error reordering pages:', error);
    return false;
  }
};

/**
 * Move a page to a new parent
 */
export const movePage = async (pageId: string | number, newParentId: string | number | null, displayOrder: number): Promise<Page | null> => {
  try {
    const response = await apiClient.post(`/documentation/pages/move`, {
      page_id: Number(pageId),
      new_parent_id: newParentId !== null ? Number(newParentId) : null,
      display_order: displayOrder,
    });
    return convertToPage(response.data);
  } catch (error) {
    logger.error('Error moving page:', error);
    return null;
  }
};

/**
 * Get pages in correct display order by parent ID
 */
export const getOrderedPagesByParentId = async (parentId: string | number): Promise<Page[]> => {
  try {
    const response = await apiClient.get(`/documentation/pages/ordered/parent/${parentId}`);
    return response.data.map(convertToPage);
  } catch (error) {
    logger.error(`Error fetching ordered pages for parent ${parentId}:`, error);
    return [];
  }
};

/**
 * Get top-level pages in correct display order
 */
export const getOrderedTopLevelPages = async (): Promise<Page[]> => {
  try {
    const response = await apiClient.get(`/documentation/pages/ordered/top-level`);
    return response.data.map(convertToPage);
  } catch (error) {
    logger.error('Error fetching ordered top-level pages:', error);
    return [];
  }
};

/**
 * Get page with ordered children
 */
export const getPageWithOrderedChildren = async (pageId: string | number): Promise<Page | null> => {
  try {
    const response = await apiClient.get(`/documentation/pages/${pageId}/with-ordered-children`);
    return convertToPage(response.data);
  } catch (error) {
    logger.error(`Error fetching page with ordered children for ${pageId}:`, error);
    return null;
  }
};

/**
 * Delete a documentation page/article
 */
export const deleteArticle = async (pageId: string | number): Promise<boolean> => {
  try {
    let numericId: number;
    
    if (typeof pageId === 'number') {
      numericId = pageId;
    } else if (!isNaN(Number(pageId))) {
      numericId = Number(pageId);
    } else {
      // If it's a slug, fetch the numeric ID first
      try {
        const response = await apiClient.get(`/documentation/pages/slug/${pageId}`);
        numericId = response.data.id;
      } catch (error) {
        logger.error(`Error fetching page with slug ${pageId}:`, error);
        return false;
      }
    }
    
    // Delete the page
    await apiClient.delete(`/documentation/pages/${numericId}`);
    return true;
  } catch (error) {
    logger.error(`Error deleting documentation page ${pageId}:`, error);
    return false;
  }
};

/**
 * Update only page metadata (title, slug, etc.) without touching content
 * Content is automatically synced via WebSocket collaboration
 */
export const updatePageMetadata = async (
  pageId: string,
  metadata: { title?: string; slug?: string; icon?: string }
): Promise<boolean> => {
  try {
    const response = await apiClient.put(
      `/documentation/pages/${pageId}/metadata`,
      metadata
    );
    return response.status === 200;
  } catch (error) {
    logger.error(`Error updating page metadata for ${pageId}:`, error);
    return false;
  }
};

/**
 * Get archived documentation pages
 */
export const getArchivedPages = async (): Promise<Page[]> => {
  try {
    const response = await apiClient.get('/documentation/pages/archived');
    if (!Array.isArray(response.data)) return [];
    return response.data.map(convertToPage);
  } catch (error) {
    logger.error('Error fetching archived pages:', error);
    return [];
  }
};

/**
 * Get trashed (soft-deleted) documentation pages
 */
export const getTrashedPages = async (): Promise<Page[]> => {
  try {
    const response = await apiClient.get('/documentation/pages/trash');
    if (!Array.isArray(response.data)) return [];
    return response.data.map(convertToPage);
  } catch (error) {
    logger.error('Error fetching trashed pages:', error);
    return [];
  }
};

/**
 * Restore a page from archive or trash back to draft
 */
export const restorePage = async (pageId: string | number): Promise<boolean> => {
  try {
    await apiClient.post(`/documentation/pages/${pageId}/restore`);
    return true;
  } catch (error) {
    logger.error(`Error restoring page ${pageId}:`, error);
    return false;
  }
};

/**
 * Permanently delete a page (hard delete, admin only)
 */
export const permanentlyDeletePage = async (pageId: string | number): Promise<boolean> => {
  try {
    await apiClient.delete(`/documentation/pages/${pageId}/permanent`);
    return true;
  } catch (error) {
    logger.error(`Error permanently deleting page ${pageId}:`, error);
    return false;
  }
};

export interface PageVisibilityResponse {
  groups: Array<{ id: number; name: string }>;
  users: Array<{ uuid: string; name: string; avatar_url?: string | null; avatar_thumb?: string | null }>;
}

/**
 * Get visibility (groups + users) for a documentation page
 */
export const getPageVisibility = async (pageId: number): Promise<PageVisibilityResponse> => {
  try {
    const response = await apiClient.get(`/documentation/pages/${pageId}/visibility`);
    return response.data;
  } catch (error) {
    logger.error(`Error fetching page visibility for page ${pageId}:`, error);
    return { groups: [], users: [] };
  }
};

/**
 * Set visibility for a documentation page (admin only)
 * Empty group_ids + user_uuids clears override (page inherits from collections)
 */
export const setPageVisibility = async (pageId: number, groupIds: number[], userUuids: string[] = []): Promise<boolean> => {
  try {
    await apiClient.put(`/documentation/pages/${pageId}/visibility`, {
      group_ids: groupIds,
      user_uuids: userUuids,
    });
    return true;
  } catch (error) {
    logger.error(`Error setting page visibility for page ${pageId}:`, error);
    return false;
  }
};

/**
 * Get subscription status for a documentation page
 */
export const getPageSubscription = async (pageId: number): Promise<boolean> => {
  try {
    const response = await apiClient.get(`/documentation/pages/${pageId}/subscription`);
    return response.data.subscribed ?? false;
  } catch (error) {
    logger.error(`Error fetching subscription status for page ${pageId}:`, error);
    return false;
  }
};

/**
 * Subscribe to a documentation page
 */
export const subscribeToPage = async (pageId: number): Promise<boolean> => {
  try {
    const response = await apiClient.post(`/documentation/pages/${pageId}/subscribe`);
    return response.data.subscribed ?? true;
  } catch (error) {
    logger.error(`Error subscribing to page ${pageId}:`, error);
    return false;
  }
};

/**
 * Unsubscribe from a documentation page
 */
export const unsubscribeFromPage = async (pageId: number): Promise<boolean> => {
  try {
    await apiClient.delete(`/documentation/pages/${pageId}/subscribe`);
    return true;
  } catch (error) {
    logger.error(`Error unsubscribing from page ${pageId}:`, error);
    return false;
  }
};

// ============================================================================
// Starred Pages
// ============================================================================

export interface StarredPageInfo {
  page_id: number;
  title: string;
  slug: string;
  icon: string | null;
  starred_at: string;
}

/**
 * Get all starred pages for the current user
 */
export const getStarredPages = async (): Promise<StarredPageInfo[]> => {
  try {
    const response = await apiClient.get('/documentation/starred');
    return response.data ?? [];
  } catch (error) {
    logger.error('Error fetching starred pages:', error);
    return [];
  }
};

/**
 * Get starred status for a documentation page
 */
export const getPageStarred = async (pageId: number): Promise<boolean> => {
  try {
    const response = await apiClient.get(`/documentation/pages/${pageId}/starred`);
    return response.data.starred ?? false;
  } catch (error) {
    logger.error(`Error fetching starred status for page ${pageId}:`, error);
    return false;
  }
};

/**
 * Star a documentation page
 */
export const starPage = async (pageId: number): Promise<boolean> => {
  try {
    const response = await apiClient.post(`/documentation/pages/${pageId}/star`);
    return response.data.starred ?? true;
  } catch (error) {
    logger.error(`Error starring page ${pageId}:`, error);
    return false;
  }
};

/**
 * Unstar a documentation page
 */
export const unstarPage = async (pageId: number): Promise<boolean> => {
  try {
    await apiClient.delete(`/documentation/pages/${pageId}/star`);
    return true;
  } catch (error) {
    logger.error(`Error unstarring page ${pageId}:`, error);
    return false;
  }
};

/**
 * Update partial page fields (title, slug, icon, status, etc.)
 */
export const updatePage = async (
  pageId: string | number,
  fields: Partial<Pick<Page, 'title' | 'slug' | 'icon' | 'status'>>
): Promise<boolean> => {
  try {
    await apiClient.put(`/documentation/pages/${pageId}`, fields);
    return true;
  } catch (error) {
    logger.error(`Error updating page ${pageId}:`, error);
    return false;
  }
};

/**
 * Archive a page (sets status to 'archived')
 */
export const archivePage = async (pageId: string | number): Promise<boolean> => {
  try {
    await apiClient.put(`/documentation/pages/${pageId}`, { status: 'archived' });
    return true;
  } catch (error) {
    logger.error(`Error archiving page ${pageId}:`, error);
    return false;
  }
};

/**
 * Export a page as markdown (returns a Blob)
 */
export const exportPageMarkdown = async (pageId: string | number): Promise<Blob | null> => {
  try {
    const response = await apiClient.get(`/documentation/pages/${pageId}/export/markdown`, {
      responseType: 'blob',
    });
    return new Blob([response.data], { type: 'text/markdown' });
  } catch (error) {
    logger.error(`Error exporting page ${pageId} as markdown:`, error);
    return null;
  }
};

export default {
  getPages,
  getAllArticles,
  getArticleById,
  getPageById,
  getPageByPath,
  searchArticles,
  saveArticle,
  createArticle,
  deleteArticle,
  getPagesByParentId,
  getPageWithChildrenByParentId,
  updateParent,
  reorderPages,
  movePage,
  getOrderedPagesByParentId,
  getOrderedTopLevelPages,
  getPageWithOrderedChildren,
  updatePageMetadata,
  getArchivedPages,
  getTrashedPages,
  restorePage,
  permanentlyDeletePage,
  getPageVisibility,
  setPageVisibility,
  getPageSubscription,
  subscribeToPage,
  unsubscribeFromPage,
  getStarredPages,
  getPageStarred,
  starPage,
  unstarPage,
  updatePage,
  archivePage,
  exportPageMarkdown,
};