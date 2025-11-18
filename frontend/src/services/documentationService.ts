import apiClient from './apiConfig';

// Note: apiClient already has baseURL set to '/api', so we don't need to prefix routes

// User info interface
export interface UserInfo {
  uuid: string;
  name: string;
}

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

// Convert backend page data to frontend Page format
export const convertToPage = (data: any): Page => {
  // Handle null or undefined data
  if (!data) {
    console.warn('Attempting to convert null or undefined data to Page');
    return {
      id: 'invalid-page',
      slug: 'invalid-page',
      title: 'Invalid Page',
      description: null,
      content: '',
      parent_id: null,
      author: 'System',
      status: 'draft',
      icon: '❓',
      children: [],
      lastUpdated: new Date().toISOString()
    };
  }

  try {
    // Clean up all properties before assigning
    const cleanId = data.id !== undefined ? data.id : `unknown-${Date.now()}`;
    const cleanSlug = typeof data.slug === 'string' ? data.slug : '';
    const cleanTitle = typeof data.title === 'string' ? data.title : 'Untitled';
    const cleanDescription = data.description !== undefined ? data.description : null;
    
    // Handle content conversion from binary (Vec<u8>) to string
    let cleanContent = '';
    if (data.content) {
      if (typeof data.content === 'string') {
        // Already a string
        cleanContent = data.content;
      } else if (Array.isArray(data.content)) {
        // It's an array of bytes from the backend
        try {
          const bytes = new Uint8Array(data.content);
          cleanContent = new TextDecoder().decode(bytes);
        } catch (decodeError) {
          console.warn('Error decoding binary content:', decodeError);
          cleanContent = '';
        }
      } else {
        cleanContent = '';
      }
    }
    
    const cleanParentId = data.parent_id !== undefined ? data.parent_id : null;
    // For backwards compatibility, fallback to created_by.name if author doesn't exist
    const cleanAuthor = typeof data.author === 'string' ? data.author : (data.created_by?.name || 'System');
    const cleanStatus = typeof data.status === 'string' ? data.status : 'published';
    const cleanIcon = typeof data.icon === 'string' ? data.icon : '📄';

    // Process children array with extra validation
    let cleanChildren: Page[] = [];
    if (Array.isArray(data.children)) {
      cleanChildren = data.children
        .filter((child: any) => child && typeof child === 'object') // Filter out non-objects
        .map((child: any) => convertToPage(child));               // Convert each valid child
    }

    return {
      id: cleanId,
      uuid: data.uuid,
      slug: cleanSlug,
      title: cleanTitle,
      description: cleanDescription,
      content: cleanContent,
      parent_id: cleanParentId,
      author: cleanAuthor,
      status: cleanStatus,
      icon: cleanIcon,
      children: cleanChildren,
      lastUpdated: data.updated_at || new Date().toISOString(),
      ticket_id: data.ticket_id || null,
      created_at: data.created_at,
      updated_at: data.updated_at,
      created_by: data.created_by,
      last_edited_by: data.last_edited_by,
      display_order: typeof data.display_order === 'number' ? data.display_order : 0
    };
  } catch (error) {
    console.error('Error converting backend page data:', error, data);
    return {
      id: `error-${Date.now()}`,
      slug: 'error-page',
      title: 'Error Page',
      description: 'An error occurred loading this page',
      content: '',
      parent_id: null,
      author: 'System',
      status: 'draft',
      icon: '⚠️',
      children: [],
      lastUpdated: new Date().toISOString()
    };
  }
};

// Convert backend page data to frontend Article format (for backward compatibility)
export const convertToArticle = (data: any): Article => {
  // Handle null or undefined data
  if (!data) {
    console.warn('Attempting to convert null or undefined data to Article');
    return {
      id: 'invalid-article',
      slug: 'invalid-article',
      title: 'Invalid Article',
      description: null,
      content: '',
      parent_id: null,
      author: 'System',
      status: 'draft',
      icon: '❓',
      children: [],
      lastUpdated: new Date().toISOString()
    };
  }

  try {
    // Use convertToPage to get a base Page object with all the sanitization
    const basePage = convertToPage(data);
    
    // Return the object as an Article (same properties as Page)
    return {
      ...basePage,
      children: basePage.children
    };
  } catch (error) {
    console.error('Error converting backend page to article:', error, data);
    return {
      id: `error-${Date.now()}`,
      slug: 'error-article',
      title: 'Error Article',
      description: 'An error occurred loading this article',
      content: '',
      parent_id: null,
      author: 'System',
      status: 'draft',
      icon: '⚠️',
      children: [],
      lastUpdated: new Date().toISOString()
    };
  }
};

/**
 * Get all top-level pages with their children
 */
export const getPages = async (): Promise<Page[]> => {
  try {
    // Fetch all pages
    const response = await apiClient.get(`/documentation/pages`);
    console.log('Raw API response for pages:', response.data);
    
    // Validate that the response is an array
    if (!Array.isArray(response.data)) {
      console.error('API response is not an array:', response.data);
      return [];
    }
    
    // Filter out any potentially invalid items before conversion
    const validItems = response.data.filter((item: any) => 
      item && typeof item === 'object' && item.id !== undefined
    );
    
    if (validItems.length !== response.data.length) {
      console.warn(`Filtered out ${response.data.length - validItems.length} invalid items from API response`);
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
        console.warn(`Page with ID ${page.id} not found in pagesMap`);
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
            console.warn(`Parent page ${page.parent_id} has no children array`);
            parentPage.children = []; // Create the children array if it doesn't exist
          }
          parentPage.children.push(pageWithChildren);
        } else {
          // If parent doesn't exist (orphaned child), add to top level
          console.warn(`Page ${page.id} has parent_id ${page.parent_id} but parent not found, adding to top level`);
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
    console.log('Pages with proper hierarchy:');
    topLevelPages.forEach((page, index) => {
      console.log(`Top level page ${index + 1}: ${page.title} (ID: ${page.id})`);
      if (page.children && page.children.length > 0) {
        console.log(`  Has ${page.children.length} children`);
        page.children.forEach((child, childIndex) => {
          console.log(`    Child ${childIndex + 1}: ${child.title} (ID: ${child.id})`);
        });
      } else {
        console.log(`  No children`);
      }
    });
    
    console.log('Organized pages hierarchy:', JSON.stringify(topLevelPages, null, 2));
    return topLevelPages;
  } catch (error) {
    console.error('Error fetching documentation pages:', error);
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
    console.error('Error fetching documentation pages:', error);
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
    console.error(`Error fetching documentation page with ID ${id}:`, error);
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
    console.error(`Error fetching documentation page with ID ${id}:`, error);
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
      console.error('Empty path provided to getPageByPath');
      return null;
    }

    // Check if the path is a numeric ID
    if (!isNaN(Number(path))) {
      try {
        console.log(`Fetching page with numeric ID: ${path}`);
        const response = await apiClient.get(`/documentation/pages/${path}`);
        return convertToPage(response.data);
      } catch (idError) {
        console.error(`Error fetching page with ID ${path}:`, idError);
        return null;
      }
    } 
    // Otherwise, treat it as a slug
    else {
      try {
        console.log(`Fetching page with slug: ${path}`);
        const response = await apiClient.get(`/documentation/pages/slug/${path}`);
        return convertToPage(response.data);
      } catch (slugError) {
        console.error(`Error fetching page with slug ${path}:`, slugError);
        return null;
      }
    }
  } catch (error) {
    console.error(`Error in getPageByPath for ${path}:`, error);
    return null;
  }
};

/**
 * Get all pages
 */
export const getAllPages = async (): Promise<Page[]> => {
  try {
    const response = await apiClient.get(`/documentation/pages`);
    const allPages = response.data;
    
    // Map for organizing children by parent ID
    const childrenMap: { [key: string]: Page[] } = {};
    
    // Array for top-level pages
    const topLevelPages: Page[] = [];
    
    // Organize pages by parent
    allPages.forEach((page: any) => {
      // Create a full Page object
      const pageObject: Page = {
        id: page.id,
        slug: page.slug || '',
        title: page.title || '',
        description: page.description || null,
        content: page.content || '',
        parent_id: page.parent_id,
        author: page.author || 'System Admin',
        status: page.status || 'published',
        icon: page.icon,
        children: [],
        lastUpdated: page.updated_at,
        ticket_id: page.ticket_id
      };
      
      // Initialize children array for this page's ID if not already done
      if (!childrenMap[page.id]) {
        childrenMap[page.id] = [];
      }
      
      if (!page.parent_id) {
        // This is a top-level page
        topLevelPages.push(pageObject);
      } else if (childrenMap[page.parent_id]) {
        // Add to parent's children
        childrenMap[page.parent_id].push(pageObject);
      } else {
        // Initialize parent's children array and add this page
        childrenMap[page.parent_id] = [pageObject];
      }
    });
    
    // Assign children to their parents
    topLevelPages.forEach(page => {
      page.children = childrenMap[page.id] || [];
    });
    
    return topLevelPages;
  } catch (error) {
    console.error('Error fetching all pages:', error);
    return [];
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
      return response.data.map((item: any) => convertToArticle(item));
    } catch (error) {
      console.error('Backend search failed, falling back to client-side search:', error);
      
      // Fallback to client-side search
      const allArticlesResponse = await apiClient.get(`/documentation/pages`);
      const allArticles = allArticlesResponse.data.map((item: any) => convertToArticle(item));
      
      // Filter articles by title and description
      const lowerQuery = query.toLowerCase();
      return allArticles.filter((article: Article) => 
        article.title.toLowerCase().includes(lowerQuery) ||
        (article.description && article.description.toLowerCase().includes(lowerQuery))
      );
    }
  } catch (searchError) {
    console.error('Error with fallback search:', searchError);
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
  console.warn('saveArticle is deprecated - content should be synced via WebSocket collaboration');
  try {
    // Determine if article ID is numeric or a slug
    let numericId: number;
    
    if (typeof article.id === 'number') {
      numericId = article.id;
    } else if (!isNaN(Number(article.id))) {
      numericId = Number(article.id);
    } else {
      // If it's a slug, we need to fetch the numeric ID first
      try {
        const response = await apiClient.get(`/documentation/pages/slug/${article.id}`);
        numericId = response.data.id;
      } catch (error) {
        console.error(`Error fetching article with slug ${article.id}:`, error);
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
    console.log('Saving article with payload:', JSON.stringify(payload));
    
    // Update the article
    const response = await apiClient.put(`/documentation/pages/${numericId}`, payload);
    
    // Fetch the updated article
    const updatedArticleResponse = await apiClient.get(`/documentation/pages/${numericId}`);
    
    // Convert backend article to frontend Page
    return convertToPage(updatedArticleResponse.data);
  } catch (error: any) {
    console.error('Error saving article:', error);
    
    // Try to log more detailed error information
    if (error.response) {
      console.error('Update error response data:', error.response.data);
      console.error('Update error response status:', error.response.status);
      
      // Try to log the request payload that caused the error
      if (error.config && error.config.data) {
        console.error('Request payload that caused error:', error.config.data);
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
    // Generate a slug from the title if not provided
    const slug = article.slug || article.title?.toLowerCase().replace(/\s+/g, '-').replace(/[^a-z0-9-]/g, '') || `page-${Date.now()}`;

    // Convert status string to enum value expected by backend
    let statusValue;
    if (typeof article.status === 'string') {
      statusValue = article.status.toLowerCase();
    } else {
      // Default to draft
      statusValue = 'draft';
    }

    // Generate a UUID for the new page
    const uuid = crypto.randomUUID();

    // Prepare the payload matching NewDocumentationPage backend struct
    const payload = {
      uuid: uuid,
      title: article.title || 'Untitled',
      slug: slug,
      icon: article.icon || '📄',
      cover_image: null,
      status: statusValue,
      created_by: uuid, // Will be overridden by backend with actual user UUID
      last_edited_by: uuid, // Will be overridden by backend with actual user UUID
      parent_id: article.parent_id !== undefined ? article.parent_id : null,
      ticket_id: article.ticket_id || null,
      display_order: article.display_order !== undefined ? article.display_order : 0,
      is_public: false,
      is_template: false,
      yjs_state_vector: null,
      yjs_document: null,
      yjs_client_id: null,
      has_unsaved_changes: false
    };

    // Print payload as a formatted string for debugging
    console.log('Creating article with payload:', JSON.stringify(payload, null, 2));

    // Create the article
    const response = await apiClient.post(`/documentation/pages`, payload);
    
    console.log('Article created successfully:', response.data);
    
    // Check if the response contains the created article
    if (!response.data || !response.data.id) {
      console.error('Invalid response data from creating article:', response.data);
      return null;
    }
    
    try {
      // Fetch the created article using the correct endpoint
      const createdArticleResponse = await apiClient.get(`/documentation/pages/${response.data.id}`);
      
      // Convert backend article to frontend Page
      return convertToPage(createdArticleResponse.data);
    } catch (fetchError) {
      console.warn('Error fetching the newly created article, returning original response:', fetchError);
      // If fetching the new article fails, return the data from the creation response
      return convertToPage(response.data);
    }
  } catch (error: any) {
    console.error('Error creating article:', error);
    if (error.response) {
      console.error('Response data:', error.response.data);
      console.error('Response status:', error.response.status);
      console.error('Response headers:', error.response.headers);
      
      // Log more details about the request
      if (error.config) {
        console.error('Request URL:', error.config.url);
        console.error('Request method:', error.config.method);
        console.error('Request data:', error.config.data);
      }
    } else if (error.request) {
      console.error('Request made but no response received:', error.request);
    } else {
      console.error('Error setting up request:', error.message);
    }
    return null;
  }
};

/**
 * Save the content of a page
 */
/**
 * @deprecated This method is deprecated. Content is now automatically synced via WebSocket collaboration.
 * Use CollaborativeEditor for content edits - it handles all sync automatically.
 */
export const savePageContent = async (pageId: string, content: string): Promise<Page | null> => {
  console.warn('savePageContent is deprecated - content should be synced via WebSocket collaboration');
  try {
    let page;
    
    // Fetch the page first to get all its data
    if (!isNaN(Number(pageId))) {
      const pageResponse = await apiClient.get(`/documentation/pages/${pageId}`);
      page = pageResponse.data;
    } else {
      const pageResponse = await apiClient.get(`/documentation/pages/slug/${pageId}`);
      page = pageResponse.data;
    }
    
    console.log('Original page data:', page);
    
    // Update the full page since there's no content-only endpoint
    try {
      // Convert status string to enum value expected by backend
      let statusValue;
      if (typeof page.status === 'string') {
        statusValue = page.status.toLowerCase();
      } else {
        // Default to published
        statusValue = 'published';
      }
      
      // Convert content string to bytes for the backend
      const contentBytes = Array.from(new TextEncoder().encode(content));
      
      // Create a clean payload object with only the required fields
      // Important: Keep created_at and updated_at exactly as they are in the original page
      // The backend expects NaiveDateTime objects, not ISO strings
      const payload = {
        slug: page.slug,
        title: page.title,
        description: page.description,
        content: contentBytes, // Send as array of bytes
        parent_id: page.parent_id,
        status: statusValue,
        icon: page.icon,
        created_at: page.created_at,
        updated_at: page.updated_at
      };
      
      // Log the payload as a JSON string to check for any issues
      console.log('Sending update payload:', JSON.stringify(payload));
      
      // Update the page using the standard update endpoint
      const response = await apiClient.put(`/documentation/pages/${page.id}`, payload);
      
      // Get the updated page
      const updatedPageResponse = await apiClient.get(`/documentation/pages/${page.id}`);
      
      // Convert backend page to frontend Page
      return convertToPage(updatedPageResponse.data);
    } catch (error: any) {
      if (error.response) {
        console.error('Update error response data:', error.response.data);
        console.error('Update error response status:', error.response.status);
        
        // Try to log the request payload that caused the error
        if (error.config && error.config.data) {
          console.error('Request payload that caused error:', error.config.data);
        }
      } else if (error.request) {
        console.error('Update error request:', error.request);
      } else {
        console.error('Update error message:', error.message);
      }
      throw error;
    }
  } catch (error) {
    console.error(`Error saving content for page ${pageId}:`, error);
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
    console.error(`Error fetching pages with parent ID ${parentId}:`, error);
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
    console.error(`Error fetching page with children for ID ${pageId}:`, error);
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
    console.error(`Error updating parent for page ${pageId}:`, error);
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
    console.error('Error reordering pages:', error);
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
    console.error('Error moving page:', error);
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
    console.error(`Error fetching ordered pages for parent ${parentId}:`, error);
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
    console.error('Error fetching ordered top-level pages:', error);
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
    console.error(`Error fetching page with ordered children for ${pageId}:`, error);
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
      // If it's a slug, we need to fetch the numeric ID first
      try {
        const response = await apiClient.get(`/documentation/pages/slug/${pageId}`);
        numericId = response.data.id;
      } catch (error) {
        console.error(`Error fetching page with slug ${pageId}:`, error);
        return false;
      }
    }
    
    // Delete the page
    await apiClient.delete(`/documentation/pages/${numericId}`);
    return true;
  } catch (error) {
    console.error(`Error deleting documentation page ${pageId}:`, error);
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
    console.error(`Error updating page metadata for ${pageId}:`, error);
    return false;
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
  savePageContent,
  getPagesByParentId,
  getPageWithChildrenByParentId,
  updateParent,
  reorderPages,
  movePage,
  getOrderedPagesByParentId,
  getOrderedTopLevelPages,
  getPageWithOrderedChildren,
  updatePageMetadata
}; 