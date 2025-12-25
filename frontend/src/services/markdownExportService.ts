import { logger } from '@/utils/logger';
import * as Y from 'yjs';
import { yDocToProsemirrorJSON } from 'y-prosemirror';
import { schema } from '@/components/editor/schema';
import { defaultMarkdownSerializer } from 'prosemirror-markdown';
import { Node } from 'prosemirror-model';
import JSZip from 'jszip';
import apiClient from './apiConfig';

interface DocumentationPageExport {
  id: number;
  uuid: string;
  title: string;
  slug: string;
  icon: string | null;
  parent_id: number | null;
  display_order: number | null;
  status: string;
  yjs_document: number[] | null;
  created_at: string;
  updated_at: string;
}

export interface ExportProgress {
  current: number;
  total: number;
  currentPage: string;
}

type ProgressCallback = (progress: ExportProgress) => void;

/**
 * Custom markdown serializer that extends the default one with our custom nodes
 */
const createMarkdownSerializer = () => {
  // Start with the default serializer's nodes and marks
  const nodes = { ...defaultMarkdownSerializer.nodes };
  const marks = { ...defaultMarkdownSerializer.marks };

  // Add custom node handlers
  nodes.ticket_link_card = (state, node) => {
    const ticketId = node.attrs.ticketId;
    const title = node.attrs.title || `Ticket #${ticketId}`;
    state.write(`[${title}](/tickets/${ticketId})\n\n`);
  };

  nodes.image = (state, node) => {
    const alt = node.attrs.alt || '';
    const src = node.attrs.src || '';
    const title = node.attrs.title ? ` "${node.attrs.title}"` : '';
    state.write(`![${alt}](${src}${title})\n\n`);
  };

  // Handle horizontal rule
  nodes.horizontal_rule = (state) => {
    state.write('---\n\n');
  };

  return { nodes, marks };
};

/**
 * Convert a Yjs document to markdown
 */
const yDocToMarkdown = (yjsDocumentBytes: number[]): string => {
  try {
    // Create a new Y.Doc and apply the update
    const ydoc = new Y.Doc();
    const update = new Uint8Array(yjsDocumentBytes);
    Y.applyUpdate(ydoc, update);

    // Check if the document has content
    const xmlFragment = ydoc.getXmlFragment('prosemirror');
    if (!xmlFragment || xmlFragment.length === 0) {
      logger.debug('Empty Yjs document');
      return '';
    }

    // Convert Y.Doc to ProseMirror JSON (pass the doc, not the fragment)
    const pmJson = yDocToProsemirrorJSON(ydoc, 'prosemirror');

    if (!pmJson || !pmJson.content) {
      logger.debug('Empty ProseMirror JSON');
      return '';
    }

    // Create a ProseMirror document from the JSON
    const doc = Node.fromJSON(schema, pmJson);

    // Serialize to markdown using our custom serializer
    const serializer = createMarkdownSerializer();
    let markdown = '';

    // Walk through the document and serialize each node
    doc.forEach((node, offset, index) => {
      markdown += serializeNode(node, serializer);
    });

    return markdown.trim();
  } catch (error) {
    logger.error('Error converting Yjs document to markdown:', error);
    return '';
  }
};

/**
 * Serialize a ProseMirror node to markdown
 */
const serializeNode = (node: Node, serializer: ReturnType<typeof createMarkdownSerializer>): string => {
  let result = '';

  if (node.isText) {
    let text = node.text || '';
    // Apply marks
    node.marks.forEach(mark => {
      if (mark.type.name === 'bold' || mark.type.name === 'strong') {
        text = `**${text}**`;
      } else if (mark.type.name === 'italic' || mark.type.name === 'em') {
        text = `*${text}*`;
      } else if (mark.type.name === 'code') {
        text = `\`${text}\``;
      } else if (mark.type.name === 'link') {
        const href = mark.attrs.href || '';
        text = `[${text}](${href})`;
      } else if (mark.type.name === 'strikethrough') {
        text = `~~${text}~~`;
      }
    });
    return text;
  }

  switch (node.type.name) {
    case 'paragraph':
      node.forEach(child => {
        result += serializeNode(child, serializer);
      });
      return result + '\n\n';

    case 'heading':
      const level = node.attrs.level || 1;
      const prefix = '#'.repeat(level) + ' ';
      node.forEach(child => {
        result += serializeNode(child, serializer);
      });
      return prefix + result + '\n\n';

    case 'bullet_list':
      node.forEach(child => {
        result += serializeNode(child, serializer);
      });
      return result;

    case 'ordered_list':
      let counter = node.attrs.order || 1;
      node.forEach(child => {
        const itemContent = serializeListItem(child, serializer, `${counter}. `);
        result += itemContent;
        counter++;
      });
      return result;

    case 'list_item':
      return serializeListItem(node, serializer, '- ');

    case 'blockquote':
      let quoteContent = '';
      node.forEach(child => {
        quoteContent += serializeNode(child, serializer);
      });
      return quoteContent.split('\n').map(line => `> ${line}`).join('\n') + '\n';

    case 'code_block':
      const lang = node.attrs.language || '';
      let code = '';
      node.forEach(child => {
        code += child.text || '';
      });
      return `\`\`\`${lang}\n${code}\n\`\`\`\n\n`;

    case 'horizontal_rule':
      return '---\n\n';

    case 'image':
      const alt = node.attrs.alt || '';
      const src = node.attrs.src || '';
      return `![${alt}](${src})\n\n`;

    case 'ticket_link_card':
      const ticketId = node.attrs.ticketId;
      const title = node.attrs.title || `Ticket #${ticketId}`;
      return `[${title}](/tickets/${ticketId})\n\n`;

    case 'hard_break':
      return '  \n';

    default:
      // For unknown nodes, try to serialize children
      node.forEach(child => {
        result += serializeNode(child, serializer);
      });
      return result;
  }
};

/**
 * Serialize a list item
 */
const serializeListItem = (node: Node, serializer: ReturnType<typeof createMarkdownSerializer>, prefix: string): string => {
  let content = '';
  let isFirst = true;

  node.forEach(child => {
    const childContent = serializeNode(child, serializer);
    if (isFirst) {
      content += prefix + childContent.trimStart();
      isFirst = false;
    } else {
      // Indent continuation lines
      const indentedContent = childContent.split('\n').map((line, i) =>
        i === 0 ? line : '  ' + line
      ).join('\n');
      content += indentedContent;
    }
  });

  return content;
};

/**
 * Create a safe filename from a title
 */
const createSafeFilename = (title: string): string => {
  return title
    .toLowerCase()
    .replace(/[^a-z0-9\s-]/g, '')
    .replace(/\s+/g, '-')
    .replace(/-+/g, '-')
    .substring(0, 100) || 'untitled';
};

/**
 * Build the folder structure for documentation pages
 */
const buildFolderStructure = (pages: DocumentationPageExport[]): Map<number, string> => {
  const pathMap = new Map<number, string>();
  const pageMap = new Map<number, DocumentationPageExport>();

  // Create a map of pages by ID
  pages.forEach(page => pageMap.set(page.id, page));

  // Build paths recursively
  const buildPath = (page: DocumentationPageExport): string => {
    if (pathMap.has(page.id)) {
      return pathMap.get(page.id)!;
    }

    const filename = createSafeFilename(page.title);

    if (page.parent_id === null) {
      const path = filename;
      pathMap.set(page.id, path);
      return path;
    }

    const parent = pageMap.get(page.parent_id);
    if (parent) {
      const parentPath = buildPath(parent);
      const path = `${parentPath}/${filename}`;
      pathMap.set(page.id, path);
      return path;
    }

    // Orphan page - put at root
    pathMap.set(page.id, filename);
    return filename;
  };

  pages.forEach(page => buildPath(page));

  return pathMap;
};

/**
 * Create frontmatter for a markdown file
 */
const createFrontmatter = (page: DocumentationPageExport): string => {
  const frontmatter = [
    '---',
    `title: "${page.title.replace(/"/g, '\\"')}"`,
    `slug: "${page.slug}"`,
  ];

  if (page.icon) {
    frontmatter.push(`icon: "${page.icon}"`);
  }

  frontmatter.push(`status: "${page.status}"`);
  frontmatter.push(`created: ${page.created_at}`);
  frontmatter.push(`updated: ${page.updated_at}`);
  frontmatter.push('---');
  frontmatter.push('');

  return frontmatter.join('\n');
};

/**
 * Fetch all documentation pages with their Yjs content
 */
const fetchDocumentationPages = async (): Promise<DocumentationPageExport[]> => {
  const response = await apiClient.get('/documentation/pages/export');
  return response.data;
};

/**
 * Export all documentation to a ZIP file of markdown files
 */
export const exportDocumentationToMarkdown = async (
  onProgress?: ProgressCallback
): Promise<Blob> => {
  logger.info('Starting documentation export to markdown');

  // Fetch all documentation pages
  const pages = await fetchDocumentationPages();
  logger.info(`Found ${pages.length} documentation pages to export`);

  if (pages.length === 0) {
    throw new Error('No documentation pages to export');
  }

  // Build folder structure
  const pathMap = buildFolderStructure(pages);

  // Create ZIP file
  const zip = new JSZip();
  const docsFolder = zip.folder('documentation');

  if (!docsFolder) {
    throw new Error('Failed to create documentation folder in ZIP');
  }

  // Process each page
  for (let i = 0; i < pages.length; i++) {
    const page = pages[i];

    // Report progress
    if (onProgress) {
      onProgress({
        current: i + 1,
        total: pages.length,
        currentPage: page.title
      });
    }

    // Get the path for this page
    const basePath = pathMap.get(page.id) || createSafeFilename(page.title);

    // Check if this page has children
    const hasChildren = pages.some(p => p.parent_id === page.id);

    // Determine the file path
    let filePath: string;
    if (hasChildren) {
      // If has children, create a folder with an index.md file
      filePath = `${basePath}/index.md`;
    } else {
      filePath = `${basePath}.md`;
    }

    // Convert content to markdown
    let markdown = '';

    // Add frontmatter
    markdown += createFrontmatter(page);

    // Add title as H1
    markdown += `# ${page.title}\n\n`;

    // Convert Yjs content to markdown if available
    if (page.yjs_document && page.yjs_document.length > 0) {
      const content = yDocToMarkdown(page.yjs_document);
      if (content) {
        markdown += content;
      }
    }

    // Add file to ZIP
    docsFolder.file(filePath, markdown);

    logger.debug(`Exported: ${filePath}`);
  }

  // Generate the ZIP file
  const blob = await zip.generateAsync({
    type: 'blob',
    compression: 'DEFLATE',
    compressionOptions: { level: 6 }
  });

  logger.info('Documentation export completed');

  return blob;
};

/**
 * Download the exported documentation
 */
export const downloadDocumentationExport = async (
  onProgress?: ProgressCallback
): Promise<void> => {
  const blob = await exportDocumentationToMarkdown(onProgress);

  // Create download link
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = `documentation-${new Date().toISOString().split('T')[0]}.zip`;
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  URL.revokeObjectURL(url);
};

export default {
  exportDocumentationToMarkdown,
  downloadDocumentationExport
};
