<script setup lang="ts">
// Collaborative Editor with Yjs for real-time document editing
//
// Logging behavior:
// - Minimal logging by default (info, warn, error only)
// - Debug logging enabled in development mode or when localStorage['editor-verbose-logging'] = 'true'
// - To enable verbose logging in production: localStorage.setItem('editor-verbose-logging', 'true')
// - To disable: localStorage.removeItem('editor-verbose-logging')

import { ref, onMounted, onBeforeUnmount, watch, computed, nextTick } from "vue";
import { useRouter } from "vue-router";
import * as Y from "yjs";
import { PermanentUserData } from "yjs";
import { WebsocketProvider } from "y-websocket";
import { EditorView } from "prosemirror-view";
import { EditorState, Selection, type Command } from "prosemirror-state";
import { schema } from "@/components/editor/schema";
import { useAuthStore } from "@/stores/auth";
import UserAvatar from "./UserAvatar.vue";
import LinkTooltip from "./editor/LinkTooltip.vue";
import RevisionHistory from "./editor/RevisionHistory.vue";
import {
    createLinkTooltipPlugin,
    showLinkTooltip,
    hideLinkTooltip,
    applyLink,
    removeLink,
    type LinkTooltipState,
} from "./editor/linkTooltipPlugin";
import { createTicketLinkPlugin, setTicketNavigationHandler } from "./editor/ticketLinkPlugin";
import {
    ySyncPlugin,
    ySyncPluginKey,
    yCursorPlugin,
    yUndoPlugin,
    undo,
    redo,
    initProseMirrorDoc,
} from "y-prosemirror";
import { keymap } from "prosemirror-keymap";
import {
    toggleMark,
    setBlockType,
    chainCommands,
    exitCode,
    createParagraphNear,
    liftEmptyBlock,
    splitBlock,
    baseKeymap,
    newlineInCode,
} from "prosemirror-commands";
import {
    wrapInList,
    splitListItem,
    liftListItem,
    sinkListItem,
} from "prosemirror-schema-list";
import "prosemirror-view/style/prosemirror.css";
import { Schema } from "prosemirror-model";

// Import individual components instead of exampleSetup
import { dropCursor } from "prosemirror-dropcursor";
// gapCursor removed - causes errors with empty Yjs documents in Chrome
import {
    inputRules,
    wrappingInputRule,
    textblockTypeInputRule,
    smartQuotes,
    emDash,
    ellipsis,
} from "prosemirror-inputrules";
import { createImageUploadPlugin } from "./editor/imageUploadPlugin";
import { syntaxHighlightPlugin } from "./editor/syntaxHighlightPlugin";
import { twemojiPlugin } from "@/plugins/prosemirror-twemoji";
import { createTicketDropIndicatorPlugin } from "./editor/ticketDropIndicatorPlugin";
import {
    createMentionPlugins,
    insertMention,
    closeMention,
    type MentionState,
    type MentionUser,
} from "@/plugins/prosemirror-mentions";
import { createMentionViewPlugin } from "@/plugins/prosemirror-mention-view";
import { useDataStore } from "@/stores/dataStore";

// Yjs awareness user state structure
interface AwarenessUser {
    name: string;
    color: string;
    uuid?: string;
    avatar?: string;
}

// Props
interface Props {
    docId: string;
    ticketId?: number;
    hideRevisionHistory?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
    hideRevisionHistory: false
});

// Get auth store for user info
const authStore = useAuthStore();
const dataStore = useDataStore();

// Set up Vue Router navigation for ticket link cards
const router = useRouter();
setTicketNavigationHandler((ticketId: number) => {
    router.push(`/tickets/${ticketId}`);
});


// Refs for template
const editorElement = ref<HTMLElement | null>(null);
const isConnected = ref(false);

// Connection status for UI display: 'connecting' | 'connected' | 'disconnected'
const connectionStatus = ref<'connecting' | 'connected' | 'disconnected'>('connecting');
const hasBeenConnected = ref(false);
let connectionTimeout: ReturnType<typeof setTimeout> | null = null;

// State for connected users
const connectedUsers = ref<{ id: string; user: AwarenessUser }[]>([]);

// Mention state
const editorWrapper = ref<HTMLElement | null>(null);
const mentionDropdownRef = ref<HTMLElement | null>(null);
const mentionState = ref<MentionState>({
    active: false,
    query: '',
    from: 0,
    to: 0,
    position: null,
});
const mentionUsers = ref<MentionUser[]>([]);
const mentionSelectedIndex = ref(0);
const isMentionSearching = ref(false);
let mentionSearchTimer: ReturnType<typeof setTimeout> | null = null;

// Remove save status tracking since backend handles saves automatically

// Track initialization state
const isInitialized = ref(false);
let reinitializeTimeout: ReturnType<typeof setTimeout> | null = null;

// Visibility change debounce timeout
let visibilityTimeout: ReturnType<typeof setTimeout> | null = null;

// Event handler references for proper cleanup
let onlineHandler: (() => void) | null = null;
let offlineHandler: (() => void) | null = null;

// WebSocket close event structure from y-websocket
interface WebSocketCloseEvent {
    code?: number;
    reason?: string;
    wasClean?: boolean;
}

// Provider event handler references for proper cleanup
let statusHandler: ((event: { status: string }) => void) | null = null;
let connectionErrorHandler: ((error: Event) => void) | null = null;
let connectionCloseHandler: ((event: WebSocketCloseEvent) => void) | null = null;
let syncedHandler: ((isSynced: boolean) => void) | null = null;
let statusReconnectHandler: ((event: { status: string }) => void) | null = null;
let awarenessChangeHandler: (() => void) | null = null;

// Revision viewing state
const isViewingRevision = ref(false);
const currentRevisionNumber = ref<number | null>(null);
const showRevisionHistory = ref(false);

// Extract ticket ID from docId (format: "ticket-123")
const ticketId = computed(() => {
    console.log('[CollaborativeEditor] docId:', props.docId);
    const match = props.docId.match(/ticket-(\d+)/);
    const id = match ? parseInt(match[1], 10) : 0;
    console.log('[CollaborativeEditor] Extracted ticketId:', id);
    return id;
});

// Toggle revision history sidebar
const toggleRevisionHistory = () => {
    showRevisionHistory.value = !showRevisionHistory.value;
};

// Custom dropdown state for toolbar
const typeMenuRef = ref<HTMLElement | null>(null);
const typeButtonRef = ref<HTMLElement | null>(null);
const insertMenuRef = ref<HTMLElement | null>(null);
const insertButtonRef = ref<HTMLElement | null>(null);
const moreMenuRef = ref<HTMLElement | null>(null);
const moreButtonRef = ref<HTMLElement | null>(null);

const showTypeMenu = ref(false);
const showInsertMenu = ref(false);
const showMoreMenu = ref(false);

// Dropdown position state (for viewport-aware positioning)
import { useDropdownPosition } from '@/composables/useDropdownPosition';

const { position: typeMenuPosition, updatePosition: updateTypeMenuPosition } =
    useDropdownPosition(typeButtonRef, showTypeMenu, { preferredWidth: 160 });
const { position: insertMenuPosition, updatePosition: updateInsertMenuPosition } =
    useDropdownPosition(insertButtonRef, showInsertMenu, { preferredWidth: 180 });
const { position: moreMenuPosition, updatePosition: updateMoreMenuPosition } =
    useDropdownPosition(moreButtonRef, showMoreMenu, { preferredWidth: 160 });

// Link tooltip state
const linkTooltipState = ref<LinkTooltipState>({
    visible: false,
    url: "",
    x: 0,
    y: 0,
    isEditing: false,
    from: 0,
    to: 0,
});

// Mention dropdown position using fixed positioning for viewport awareness
const mentionDropdownStyle = computed(() => {
    if (!mentionState.value.active || !mentionState.value.position) {
        return { display: 'none' };
    }

    const { top, left } = mentionState.value.position;
    const dropdownHeight = 280;
    const dropdownWidth = 300;
    const viewportHeight = window.innerHeight;
    const viewportWidth = window.innerWidth;
    const padding = 8;

    // Check if dropdown would overflow bottom of viewport
    const wouldOverflowBottom = top + dropdownHeight + padding > viewportHeight;

    // Calculate left position, ensuring it doesn't overflow right edge
    const adjustedLeft = Math.min(left, viewportWidth - dropdownWidth - padding);

    return {
        display: 'block',
        position: 'fixed',
        top: wouldOverflowBottom ? 'auto' : `${top + padding}px`,
        bottom: wouldOverflowBottom ? `${viewportHeight - top + padding}px` : 'auto',
        left: `${Math.max(padding, adjustedLeft)}px`,
    };
});

// Mention handlers
const searchMentionUsers = async (query: string) => {
    isMentionSearching.value = true;
    try {
        const result = await dataStore.getPaginatedUsers({
            page: 1,
            pageSize: 8,
            search: query || undefined,
        });
        mentionUsers.value = result.data as MentionUser[];
        mentionSelectedIndex.value = 0;
    } catch (error) {
        console.error('Failed to search users:', error);
        mentionUsers.value = [];
    } finally {
        isMentionSearching.value = false;
    }
};

const debouncedMentionSearch = (query: string) => {
    if (mentionSearchTimer) clearTimeout(mentionSearchTimer);
    mentionSearchTimer = setTimeout(() => searchMentionUsers(query), 150);
};

const handleMentionStateChange = (state: MentionState) => {
    mentionState.value = state;
    if (state.active) {
        debouncedMentionSearch(state.query);
    }
};

const selectMentionUser = (user: MentionUser) => {
    if (!editorView) return;
    insertMention(editorView, user, schema.nodes.mention);
};

// Handle keyboard navigation in mention dropdown (called by ProseMirror plugin)
// Returns true if the key was handled to prevent default ProseMirror behavior
const handleMentionKeyDown = (key: 'ArrowUp' | 'ArrowDown' | 'Enter' | 'Tab' | 'Escape'): boolean => {
    if (!mentionState.value.active || !editorView) return false;

    switch (key) {
        case 'ArrowDown':
            mentionSelectedIndex.value = Math.min(mentionSelectedIndex.value + 1, mentionUsers.value.length - 1);
            scrollMentionToSelected();
            return true;
        case 'ArrowUp':
            mentionSelectedIndex.value = Math.max(mentionSelectedIndex.value - 1, 0);
            scrollMentionToSelected();
            return true;
        case 'Enter':
        case 'Tab':
            if (mentionUsers.value.length > 0) {
                selectMentionUser(mentionUsers.value[mentionSelectedIndex.value]);
                return true;
            }
            return false;
        case 'Escape':
            closeMention(editorView);
            return true;
    }
    return false;
};

const scrollMentionToSelected = () => {
    nextTick(() => {
        const dropdown = mentionDropdownRef.value;
        if (!dropdown) return;
        const selected = dropdown.querySelector('.selected') as HTMLElement;
        if (selected) {
            selected.scrollIntoView({ block: 'nearest' });
        }
    });
};

// Global variables - mirroring the demo approach exactly
let ydoc: Y.Doc | null = null;
let provider: WebsocketProvider | null = null;
let yXmlFragment: Y.XmlFragment | null = null;
let editorView: EditorView | null = null;
let permanentUserData: SafePermanentUserData | null = null;

// Create a wrapper around PermanentUserData that provides fallback for missing users
class SafePermanentUserData {
    private pud: Y.PermanentUserData;

    constructor(doc: Y.Doc) {
        this.pud = new Y.PermanentUserData(doc);
    }

    setUserMapping(doc: Y.Doc, clientId: number, userId: string) {
        this.pud.setUserMapping(doc, clientId, userId);
    }

    getUserByClientId(clientId: number) {
        const user = this.pud.getUserByClientId(clientId);
        // If user not found, return a default anonymous user instead of null
        if (user === null || user === undefined) {
            return `User-${clientId}`;
        }
        return user;
    }

    getUserByDeletedId(id: { visibleUsers: Set<unknown>; visibleDs: unknown }) {
        const user = this.pud.getUserByDeletedId(id);
        // If user not found, return a default anonymous user instead of null
        if (user === null || user === undefined) {
            return 'Unknown User';
        }
        return user;
    }

    // Expose dss property that y-prosemirror uses
    get dss() {
        return this.pud.dss;
    }
}

// Enhanced logging
const log = {
    info: (message: string, ...args: unknown[]) =>
        console.log(`[YJS-Editor] ${message}`, ...args),
    error: (message: string, ...args: unknown[]) =>
        console.error(`[YJS-Editor] ${message}`, ...args),
    debug: (message: string, ...args: unknown[]) => {
        // Only log debug messages in development or when verbose logging is enabled
        if (
            import.meta.env.DEV ||
            window.localStorage.getItem("editor-verbose-logging") === "true"
        ) {
            console.debug(`[YJS-Editor] ${message}`, ...args);
        }
    },
    warn: (message: string, ...args: unknown[]) =>
        console.warn(`[YJS-Editor] ${message}`, ...args),
};

// Helper function to get WebSocket state text
const getWebSocketStateText = (readyState: number): string => {
    switch (readyState) {
        case WebSocket.CONNECTING:
            return "CONNECTING";
        case WebSocket.OPEN:
            return "OPEN";
        case WebSocket.CLOSING:
            return "CLOSING";
        case WebSocket.CLOSED:
            return "CLOSED";
        default:
            return `UNKNOWN(${readyState})`;
    }
};

// Helper function to get close code meaning
const getCloseCodeMeaning = (code: number): string => {
    switch (code) {
        case 1000:
            return "Normal closure";
        case 1001:
            return "Going away";
        case 1002:
            return "Protocol error";
        case 1003:
            return "Unsupported data";
        case 1004:
            return "Reserved";
        case 1005:
            return "No status received";
        case 1006:
            return "Abnormal closure";
        case 1007:
            return "Invalid frame payload data";
        case 1008:
            return "Policy violation";
        case 1009:
            return "Message too big";
        case 1010:
            return "Mandatory extension";
        case 1011:
            return "Internal server error";
        case 1012:
            return "Service restart";
        case 1013:
            return "Try again later";
        case 1014:
            return "Bad gateway";
        case 1015:
            return "TLS handshake";
        default:
            return `Unknown code (${code})`;
    }
};

// Create custom input rules function to replace exampleSetup
const buildInputRules = (schema: Schema) => {
    const rules = [];

    // Heading rules: # for h1, ## for h2, etc.
    if (schema.nodes.heading) {
        for (let i = 1; i <= 6; i++) {
            rules.push(
                textblockTypeInputRule(
                    new RegExp(`^(#{${i}})\\s$`),
                    schema.nodes.heading,
                    { level: i },
                ),
            );
        }
    }

    // Blockquote rule: > followed by space
    if (schema.nodes.blockquote) {
        rules.push(wrappingInputRule(/^\s*>\s$/, schema.nodes.blockquote));
    }

    // Code block rules
    if (schema.nodes.code_block) {
        // Basic code block: ``` followed by Enter
        rules.push(textblockTypeInputRule(/^```$/, schema.nodes.code_block));

        // Code block with language: ```language
        rules.push(
            textblockTypeInputRule(
                /^```(\w+)\s$/,
                schema.nodes.code_block,
                (match) => ({ language: match[1] }),
            ),
        );
    }

    // List rules
    if (schema.nodes.bullet_list) {
        // Bullet list: * or - or + followed by space
        // More permissive rule to catch various list markers
        rules.push(
            wrappingInputRule(/^\s*([-*+])\s$/, schema.nodes.bullet_list),
        );
    }

    if (schema.nodes.ordered_list) {
        // Ordered list: 1. followed by space
        // Allow any digit sequence followed by period or right parenthesis
        rules.push(
            wrappingInputRule(
                /^\s*(\d+)[.)]\s$/,
                schema.nodes.ordered_list,
                (match) => ({ order: +match[1] }),
                (match, node) =>
                    node.childCount + node.attrs.order === +match[1],
            ),
        );
    }

    // Smart quotes, ellipsis, em-dash
    rules.push(...smartQuotes, ellipsis, emDash);

    return inputRules({ rules });
};

// Create custom keymap for list behaviors
const createListKeymap = (schema: Schema) => {
    const keys: { [key: string]: Command } = {};

    // Add key bindings for list behavior
    if (schema.nodes.bullet_list && schema.nodes.list_item) {
        // Add Enter key handling for bullet lists - this makes lists continue when pressing Enter
        keys["Enter"] = splitListItem(schema.nodes.list_item);

        // Tab to indent list items (increase nesting level)
        keys["Tab"] = sinkListItem(schema.nodes.list_item);

        // Shift-Tab to outdent list items (decrease nesting level)
        keys["Shift-Tab"] = liftListItem(schema.nodes.list_item);

        // Add keyboard shortcuts for toggling lists
        keys["Mod-Shift-8"] = wrapInList(schema.nodes.bullet_list); // Ctrl+Shift+8 for bullet list

        if (schema.nodes.ordered_list) {
            keys["Mod-Shift-9"] = wrapInList(schema.nodes.ordered_list); // Ctrl+Shift+9 for ordered list
        }
    }

    return keys;
};

// Initialize editor following the official Yjs demo pattern
const initEditor = async () => {
    if (!editorElement.value) return;

    try {
        log.info("Initializing collaborative editor with docId:", props.docId);

        // 1. Create new Yjs document
        ydoc = new Y.Doc();
        ydoc.gc = false;  // Disable garbage collection to preserve all document history

        // DIAGNOSTIC: Log ALL ydoc updates to trace where sync breaks
        // This listener fires whenever the document changes locally OR remotely
        ydoc.on('update', (update: Uint8Array, origin: unknown) => {
            const originObj = origin as { constructor?: { name?: string } } | null;
            const isLocal = origin === null || origin === ydoc.clientID || (originObj?.constructor?.name === 'WebsocketProvider' ? false : true);
            log.info('🔄 YDOC UPDATE EVENT:', {
                updateSize: update.length,
                origin: originObj?.constructor?.name || String(origin) || 'null',
                isLikelyLocal: isLocal,
                yXmlFragmentLength: yXmlFragment?.length || 0,
                clientId: ydoc.clientID,
                timestamp: new Date().toISOString(),
            });

            // Log update bytes for debugging
            if (update.length < 100) {
                log.debug('   Update bytes:', Array.from(update).map(b => b.toString(16).padStart(2, '0')).join(' '));
            }
        });

        // 1.5. Create SafePermanentUserData to track user contributions across snapshots
        // This wrapper provides fallback values for missing users instead of returning null
        permanentUserData = new SafePermanentUserData(ydoc);

        // 2. Create the websocket provider
        // Note: y-websocket automatically appends `/${docId}` to the provided URL
        // Provide base URL without the document ID
        // Derive WebSocket URL from API URL for consistency with REST API configuration
        const apiUrl = import.meta.env.VITE_API_URL || '/api';
        let baseWsUrl = import.meta.env.VITE_WS_SERVER_URL;

        if (!baseWsUrl) {
            if (apiUrl.startsWith('/')) {
                // Relative path - use current origin with WebSocket protocol
                const wsProtocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
                baseWsUrl = `${wsProtocol}//${window.location.host}${apiUrl}/collaboration/ws`;
            } else {
                // Absolute URL - convert http(s) to ws(s)
                baseWsUrl = apiUrl.replace(/^http/, 'ws') + '/collaboration/ws';
            }
        }

        // Check authentication using auth store (httpOnly cookies)
        const authStore = useAuthStore();
        if (!authStore.isAuthenticated) {
            log.error("No authentication token found. Please log in.");
            return;
        }

        log.debug("WebSocket connection details:", {
            baseUrl: baseWsUrl,
            documentId: props.docId,
            isAuthenticated: authStore.isAuthenticated,
        });

        // Note: Authentication is handled via httpOnly cookies automatically
        // No need to pass token - WebSocket upgrade request includes cookies

        // Create WebsocketProvider with custom URL construction
        // y-websocket will append /${docId} to the baseWsUrl, creating the correct backend route
        provider = new WebsocketProvider(baseWsUrl, props.docId, ydoc, {
            // Set resync interval to 20 seconds to prevent 30-second timeout disconnects
            // y-websocket closes connection if no message received in 30s
            // Periodic Yjs protocol messages keep the connection alive
            resyncInterval: 20000, // 20 seconds
            // Disable broadcast channel for now to reduce complexity
            disableBc: true,
        });

        // 3. Set base awareness information for user identification
        provider.awareness.setLocalState({
            user: {
                name: getUserDisplayName(),
                color: getRandomColor(),
                uuid: authStore.user?.uuid || undefined,
                avatar: authStore.user?.avatar_thumb || authStore.user?.avatar_url || undefined,
            },
        });

        // 3.5. Map the Yjs client ID to the user UUID for snapshot tracking
        if (permanentUserData && authStore.user?.uuid) {
            permanentUserData.setUserMapping(ydoc, ydoc.clientID, authStore.user.uuid);
            log.info(`Mapped client ID ${ydoc.clientID} to user ${authStore.user.uuid}`);
        }

        // 4. Get the XML fragment and initialize ProseMirror document
        // IMPORTANT: This must be done BEFORE the WebSocket sync happens
        // so that the ySyncPlugin is attached and can process incoming updates
        yXmlFragment = ydoc.getXmlFragment("prosemirror");

        // Initialize ProseMirror with the Yjs binding
        const { doc, mapping } = initProseMirrorDoc(yXmlFragment, schema);

        // Verify the document was initialized correctly
        if (!doc) {
            throw new Error(
                "Failed to initialize ProseMirror document from Yjs",
            );
        }

        // 5. Create the editor view BEFORE sync completes
        // The ySyncPlugin must be attached to receive and process sync messages
        if (!editorElement.value) {
            throw new Error("Editor element became null during initialization");
        }

        editorView = new EditorView(editorElement.value, {
            state: EditorState.create({
                doc: doc,
                schema,
                plugins: [
                    ySyncPlugin(yXmlFragment, {
                        mapping,
                        // Use PermanentUserData instance populated with user mappings
                        // Allows snapshot rendering to lookup users by client ID
                        // Type cast needed: y-prosemirror expects yjs.PermanentUserData but our
                        // SafePermanentUserData wrapper provides fallback values for missing users
                        permanentUserData: permanentUserData as unknown as typeof PermanentUserData.prototype
                    }),
                    yCursorPlugin(provider.awareness, {
                        // Custom cursor builder that handles missing users gracefully
                        cursorBuilder: (user: AwarenessUser | undefined, clientId: number): HTMLElement => {
                            const cursor = document.createElement('span');
                            cursor.classList.add('ProseMirror-yjs-cursor');
                            cursor.setAttribute('style', `border-color: ${user?.color || '#808080'}`);
                            const userLabel = document.createElement('div');
                            userLabel.setAttribute('style', `background-color: ${user?.color || '#808080'}`);
                            userLabel.textContent = user?.name || 'Anonymous';
                            cursor.appendChild(userLabel);
                            return cursor;
                        },
                        // Handle missing users gracefully (e.g., when viewing snapshots)
                        getClientColor: (clientId: number) => {
                            const user = provider?.awareness.getStates().get(clientId);
                            if (user && user.user) {
                                return user.user.color || '#808080';
                            }
                            // Return a default color for missing/historical users
                            return '#808080';
                        }
                    }),
                    yUndoPlugin(),
                    createLinkTooltipPlugin({
                        onStateChange: (state) => {
                            linkTooltipState.value = state;
                        },
                    }),
                    createTicketLinkPlugin(),
                    keymap({
                        "Mod-z": undo,
                        "Mod-y": redo,
                        "Mod-Shift-z": redo,
                        "Mod-b": toggleMark(schema.marks.strong),
                        "Mod-i": toggleMark(schema.marks.em),
                        "Mod-k": showLinkTooltip(true), // Cmd+K to add/edit link
                        "Mod-Alt-c": setBlockType(schema.nodes.code_block),
                        // Exit code block with triple backticks
                        "```": (state, dispatch) => {
                            const { $from } = state.selection;
                            if (
                                $from.parent.type === schema.nodes.code_block &&
                                dispatch
                            ) {
                                // Check if at the end of a code block
                                const after = $from.after();
                                const tr = state.tr.replaceWith(
                                    after,
                                    after,
                                    schema.nodes.paragraph.createAndFill()!,
                                );
                                tr.setSelection(
                                    Selection.near(tr.doc.resolve(after + 1)),
                                );
                                dispatch(tr);
                                return true;
                            }
                            return false;
                        },
                        // Mod-Enter to exit code block (standard ProseMirror pattern)
                        "Mod-Enter": exitCode,
                        // Enter handling in code blocks - exit on empty trailing line
                        Enter: (state, dispatch, view) => {
                            const { $from } = state.selection;
                            if ($from.parent.type !== schema.nodes.code_block) {
                                return false;
                            }

                            // Use view.endOfTextblock for accurate position detection
                            const atEnd = view
                                ? view.endOfTextblock("forward")
                                : $from.parentOffset === $from.parent.content.size;

                            // Check if the last line is empty (content ends with newline or is empty)
                            const content = $from.parent.textContent;
                            const lastLineEmpty = content.length === 0 || content.endsWith("\n");

                            if (atEnd && lastLineEmpty && dispatch) {
                                // Exit the code block and create a paragraph after it
                                const after = $from.after();
                                let tr = state.tr;

                                // Remove the trailing newline if present
                                if (content.endsWith("\n")) {
                                    tr = tr.delete($from.pos - 1, $from.pos);
                                }

                                // Insert a new paragraph after the code block
                                // Use insert instead of replaceWith - works better at end of document
                                const insertPos = tr.mapping.map(after);
                                tr = tr.insert(insertPos, schema.nodes.paragraph.createAndFill()!);

                                // Move cursor into the new paragraph
                                tr.setSelection(Selection.near(tr.doc.resolve(insertPos + 1)));
                                dispatch(tr);
                                return true;
                            }

                            // Otherwise, insert a newline within the code block
                            if (dispatch) {
                                dispatch(state.tr.insertText("\n"));
                            }
                            return true;
                        },
                        // ArrowUp at start of code block at document start - insert paragraph above
                        ArrowUp: (state, dispatch, view) => {
                            const { $from } = state.selection;
                            if ($from.parent.type !== schema.nodes.code_block) {
                                return false;
                            }

                            // Use view.endOfTextblock for accurate position detection
                            const atStart = view
                                ? view.endOfTextblock("backward")
                                : $from.parentOffset === 0;

                            // Check if code block is at the start of the document
                            const before = $from.before();
                            const isFirstBlock = before === 1;

                            if (atStart && isFirstBlock && dispatch) {
                                // Insert a paragraph before the code block
                                const tr = state.tr.insert(
                                    before,
                                    schema.nodes.paragraph.createAndFill()!
                                );
                                tr.setSelection(Selection.near(tr.doc.resolve(before)));
                                dispatch(tr);
                                return true;
                            }
                            return false;
                        },
                        // Backspace in empty code block - delete it and replace with paragraph
                        Backspace: (state, dispatch) => {
                            const { $from } = state.selection;
                            if ($from.parent.type !== schema.nodes.code_block) {
                                return false;
                            }

                            // Only handle empty code blocks
                            if ($from.parent.content.size !== 0) {
                                return false;
                            }

                            if (dispatch) {
                                const before = $from.before();
                                const after = $from.after();
                                const tr = state.tr.replaceWith(
                                    before,
                                    after,
                                    schema.nodes.paragraph.createAndFill()!
                                );
                                tr.setSelection(Selection.near(tr.doc.resolve(before + 1)));
                                dispatch(tr);
                            }
                            return true;
                        },
                    }),
                    // Add list handling keymap - this is crucial for proper list behavior
                    keymap(createListKeymap(schema)),
                    // Add individual plugins instead of exampleSetup
                    buildInputRules(schema), // Custom markdown input rules
                    // Mention plugins must come BEFORE baseKeymap to intercept Enter/Tab/Arrow keys
                    ...createMentionPlugins({
                        onStateChange: handleMentionStateChange,
                        onKeyDown: handleMentionKeyDown,
                    }),
                    keymap(baseKeymap), // Basic key bindings
                    dropCursor(), // Shows cursor when dragging
                    createTicketDropIndicatorPlugin(), // Shows drop indicator for ticket cards
                    // NOTE: gapCursor() removed - causes null reference errors with empty Yjs documents
                    createImageUploadPlugin({
                        ticketId: props.ticketId,
                        onUploadStart: () => log.debug('Image upload started'),
                        onUploadEnd: () => log.debug('Image upload completed'),
                        onUploadError: (error) => log.error('Image upload failed:', error)
                    }),
                    syntaxHighlightPlugin,
                    createMentionViewPlugin(),
                    twemojiPlugin,
                ],
            }),
        });

        // Load initial users for mentions
        searchMentionUsers('');

        // 7. Set up connection status handler with enhanced logging
        // Store handler reference for proper cleanup
        statusHandler = (event: {
            status: "connected" | "disconnected" | "connecting";
        }) => {
            const previousStatus = isConnected.value
                ? "connected"
                : "disconnected";
            isConnected.value = event.status === "connected";

            // Only log status changes, not every status event
            if (previousStatus !== event.status) {
                log.info(`Connection status: ${event.status}`);
            }

            // Update connectionStatus for UI display
            if (event.status === "connected") {
                connectionStatus.value = 'connected';
                hasBeenConnected.value = true;
                // Clear the connection timeout now that connection is established
                if (connectionTimeout) {
                    clearTimeout(connectionTimeout);
                    connectionTimeout = null;
                }
                log.info("WebSocket connected successfully");
            } else if (event.status === "disconnected") {
                // Only show disconnected if previously connected
                // Otherwise keep showing "connecting" until the 10-second timeout
                if (hasBeenConnected.value) {
                    connectionStatus.value = 'disconnected';
                }
                log.warn(
                    "WebSocket disconnected - will attempt to reconnect automatically",
                );

                // Only run diagnostics in debug mode or when explicitly enabled
                if (
                    import.meta.env.DEV ||
                    window.localStorage.getItem(
                        "editor-verbose-logging",
                    ) === "true"
                ) {
                    diagnoseConnectionIssue();
                }
            } else if (event.status === "connecting") {
                // If previously connected and now reconnecting, show connecting
                if (hasBeenConnected.value) {
                    connectionStatus.value = 'connecting';
                }
            }
        };
        provider.on("status", statusHandler);

        // Add error event handler for more detailed error information
        // Store handler reference for proper cleanup
        connectionErrorHandler = (error: Event) => {
            log.error("WebSocket connection error:", error);
            const errorEvent = error as Event & { message?: string; code?: number; type?: string; target?: unknown };
            log.debug("Error details:", {
                message: errorEvent.message || "No error message",
                code: errorEvent.code || "No error code",
                type: errorEvent.type || "No error type",
                target: errorEvent.target || "No target info",
                timestamp: new Date().toISOString(),
            });
        };
        provider.on("connection-error", connectionErrorHandler);

        // Monitor for authentication-related disconnections
        // Store handler reference for proper cleanup
        connectionCloseHandler = (event: WebSocketCloseEvent) => {
            log.warn("WebSocket connection closed:", {
                code: event.code,
                reason: event.reason,
                wasClean: event.wasClean,
                timestamp: new Date().toISOString(),
            });

            // Check for authentication-related close codes
            if (event.code === 1008) {
                log.error(
                    "WebSocket closed due to policy violation - likely authentication failure",
                );
                log.error(
                    "Check if JWT token is valid and user still exists in database",
                );
            } else if (event.code === 1011) {
                log.error(
                    "WebSocket closed due to server error - likely backend database/processing issue",
                );
            } else if (event.code === 1006) {
                log.warn(
                    "WebSocket closed abnormally - network issue or server crash",
                );
            }
        };
        provider.on("connection-close", connectionCloseHandler);

        // Monitor initial sync completion
        // Store handler reference for proper cleanup
        syncedHandler = (isSynced: boolean) => {
            log.info("🔄 WebSocket sync state changed:", {
                isSynced,
                yXmlFragmentLength: yXmlFragment?.length || 0,
                editorContent: editorView?.state.doc.textContent || "(empty)",
                editorContentLength: editorView?.state.doc.textContent.length || 0,
            });

            if (isSynced && yXmlFragment && editorView) {
                const pmText = editorView.state.doc.textContent;
                log.info("✅ Initial sync complete - Content check:", {
                    yXmlLength: yXmlFragment.length,
                    pmContentLength: pmText.length,
                    pmTextPreview: pmText.substring(0, 100),
                });
            }
        };
        provider.on("synced", syncedHandler);

        // Note: Intentionally NOT overriding provider.ws.onmessage here.
        // y-websocket handles all sync messages (SYNC_STEP_1, SYNC_STEP_2, SYNC_UPDATE)
        // internally through its messageHandlers. Overriding onmessage can interfere
        // with the sync protocol and cause document content to not be applied correctly.

        // Track sync protocol errors which can cause disconnections
        // Monitor document updates to verify content is syncing
        ydoc.on("updateV2", (update: Uint8Array) => {
            log.debug("📨 Yjs document update received:", {
                updateSize: update.length,
                yXmlFragmentLength: yXmlFragment?.length || 0,
                editorContent: editorView?.state.doc.textContent || "(empty)",
                timestamp: new Date().toISOString(),
            });

            // If content exists in Yjs but not in editor, log a warning
            if (yXmlFragment && yXmlFragment.length > 0 && editorView) {
                const pmContent = editorView.state.doc.textContent;
                if (!pmContent || pmContent.length === 0) {
                    log.warn("⚠️ Content exists in Yjs but not visible in ProseMirror editor!");
                    log.warn("yXmlFragment length:", yXmlFragment.length);
                    log.warn("ProseMirror content:", pmContent);
                }
            }
        });

        // Add retry logic monitoring - simplified
        let reconnectAttempts = 0;
        const maxReconnectAttempts = 5;
        let reconnectTimeout: ReturnType<typeof setTimeout> | null = null;

        // Store handler reference for proper cleanup
        statusReconnectHandler = (event: {
            status: "connected" | "disconnected" | "connecting";
        }) => {
            if (event.status === "connecting") {
                reconnectAttempts++;
                // Only log after several attempts to avoid noise
                if (reconnectAttempts > 2) {
                    log.warn(
                        `Reconnection attempt ${reconnectAttempts}/${maxReconnectAttempts}`,
                    );
                }

                if (reconnectAttempts > maxReconnectAttempts) {
                    log.error(
                        "Max reconnection attempts exceeded - connection failed",
                    );
                    log.error(
                        "Possible causes: server down, token expired, or network issues",
                    );
                    // Stop trying to reconnect
                    return;
                }
            } else if (event.status === "connected") {
                reconnectAttempts = 0; // Reset counter on successful connection
                if (reconnectTimeout) {
                    clearTimeout(reconnectTimeout);
                    reconnectTimeout = null;
                }
                log.info("WebSocket connected successfully");
            } else if (event.status === "disconnected") {
                // Add delay before allowing reconnection to prevent rapid cycling
                if (reconnectTimeout) {
                    clearTimeout(reconnectTimeout);
                }
                reconnectTimeout = setTimeout(() => {
                    if (reconnectAttempts < maxReconnectAttempts) {
                        log.warn(
                            "WebSocket disconnected - will attempt to reconnect automatically",
                        );
                    }
                }, 2000); // Wait 2 seconds before allowing reconnection
            }
        };
        provider.on("status", statusReconnectHandler);

        // 7. Add awareness change listener to update connected users
        // Store handler reference for proper cleanup
        awarenessChangeHandler = () => {
            updateConnectedUsers();
        };
        provider.awareness.on("change", awarenessChangeHandler);

        // 8. Note: Save status tracking removed since backend handles saves automatically via Redis

        // 9. For debugging purposes
        window.example = {
            provider,
            ydoc,
            yXmlFragment,
            editorView,
            diagnoseConnection: diagnoseConnectionIssue,
        };

        // Add direct WebSocket event monitoring - ALWAYS monitor close events
        // to diagnose disconnection issues
        if (provider && provider.ws) {
            const originalOnClose = provider.ws.onclose;
            provider.ws.onclose = (event: CloseEvent) => {
                // Always log WebSocket close events as errors for debugging
                // This helps identify why connections are closing prematurely
                log.error("[DIAGNOSTIC] WebSocket closed!", {
                    code: event.code,
                    reason: event.reason || "No reason provided",
                    wasClean: event.wasClean,
                    closeCodeMeaning: getCloseCodeMeaning(event.code),
                    timestamp: new Date().toISOString(),
                    isDocumentHidden: document.hidden,
                    providerState: {
                        wsconnected: provider?.wsconnected,
                        wsconnecting: provider?.wsconnecting,
                    },
                    docId: props.docId,
                    yXmlFragmentLength: yXmlFragment?.length || 0,
                    editorContent: editorView?.state.doc.textContent?.substring(0, 100) || "(empty)",
                });

                // Log stack trace to identify caller
                log.debug("WebSocket close stack trace:", new Error().stack);

                // Call original handler if it exists
                if (originalOnClose && provider?.ws) {
                    originalOnClose.call(provider.ws, event);
                }
            };

            const originalOnError = provider.ws.onerror;
            provider.ws.onerror = (event: Event) => {
                log.error("WebSocket error event:", {
                    type: event.type,
                    timestamp: new Date().toISOString(),
                });

                // Call original handler if it exists
                if (originalOnError && provider?.ws) {
                    originalOnError.call(provider.ws, event);
                }
            };
        }

        log.info("Editor initialized successfully");
        isInitialized.value = true;
    } catch (error) {
        log.error("Error initializing editor:", error);
        // Clean up on error
        cleanup();
        // Retry after a short delay if this was a transient error
        if (!isInitialized.value) {
            log.warn("Retrying editor initialization in 2 seconds...");
            setTimeout(() => {
                initEditor();
            }, 2000);
        }
    }
};

// Helper function to get random color for user
const getRandomColor = () => {
    const colors = [
        "#f87171",
        "#fb923c",
        "#fbbf24",
        "#a3e635",
        "#34d399",
        "#22d3ee",
        "#60a5fa",
        "#a78bfa",
    ];
    return colors[Math.floor(Math.random() * colors.length)];
};

// Helper function to get user display name
const getUserDisplayName = () => {
    if (!authStore.user) {
        return "Guest " + Math.floor(Math.random() * 1000);
    }

    // Use the user's name from the auth store
    return authStore.user.name;
};

// Simple function to update connected users UI
const updateConnectedUsers = () => {
    if (!provider) return;

    try {
        const states = provider.awareness.getStates();
        const users: { id: string; user: AwarenessUser }[] = [];

        // Convert Map to array and exclude the current user
        states.forEach((state, clientId) => {
            if (
                state &&
                state.user &&
                provider &&
                clientId !== provider.awareness.clientID
            ) {
                const awarenessUser = state.user as AwarenessUser;
                // Only include users with valid user data
                if (awarenessUser.name && typeof awarenessUser.name === "string") {
                    users.push({
                        id: String(clientId),
                        user: awarenessUser,
                    });
                }
            }
        });

        connectedUsers.value = users;
    } catch (error) {
        log.error("Error updating connected users:", error);
    }
};

// Focus the editor only when clicking on empty container areas (not on ProseMirror content)
// This prevents interference with ProseMirror's native touch/click handling on mobile
const focusEditor = (event: MouseEvent | TouchEvent) => {
    if (!editorView) return;

    // Check if the click/tap target is the editor container itself (not the ProseMirror content)
    // ProseMirror handles focus internally when you click on its content
    const target = event.target as HTMLElement;
    const proseMirrorElement = editorView.dom;

    // Only manually focus if clicking outside the ProseMirror element
    // (e.g., on padding areas of the container)
    if (!proseMirrorElement.contains(target)) {
        editorView.focus();
    }
};

// Event listeners for click outside
const handleClickOutside = (event: MouseEvent) => {
    const target = event.target as Node;

    // Handle Type menu
    if (showTypeMenu.value && typeMenuRef.value && typeButtonRef.value) {
        if (
            !typeMenuRef.value.contains(target) &&
            !typeButtonRef.value.contains(target)
        ) {
            showTypeMenu.value = false;
        }
    }

    // Handle Insert menu
    if (showInsertMenu.value && insertMenuRef.value && insertButtonRef.value) {
        if (
            !insertMenuRef.value.contains(target) &&
            !insertButtonRef.value.contains(target)
        ) {
            showInsertMenu.value = false;
        }
    }

    // Handle More menu
    if (showMoreMenu.value && moreMenuRef.value && moreButtonRef.value) {
        if (
            !moreMenuRef.value.contains(target) &&
            !moreButtonRef.value.contains(target)
        ) {
            showMoreMenu.value = false;
        }
    }
};

const handleKeydown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
        if (showTypeMenu.value) {
            showTypeMenu.value = false;
            typeButtonRef.value?.focus();
        }
        if (showInsertMenu.value) {
            showInsertMenu.value = false;
            insertButtonRef.value?.focus();
        }
        if (showMoreMenu.value) {
            showMoreMenu.value = false;
            moreButtonRef.value?.focus();
        }
    }
};

// Handle tab visibility changes with debounce to prevent aggressive disconnection
// When browser backgrounds tab for extended periods, disconnect to save resources
// Short tab switches (< 30 seconds) should maintain the connection
const handleVisibilityChange = () => {
    // Clear any pending visibility timeout
    if (visibilityTimeout) {
        clearTimeout(visibilityTimeout);
        visibilityTimeout = null;
    }

    if (document.hidden && provider?.wsconnected) {
        // Wait 30 seconds before disconnecting when backgrounded
        // This prevents disconnect during brief tab switches
        visibilityTimeout = setTimeout(() => {
            if (document.hidden && provider?.wsconnected) {
                log.info("Tab backgrounded for 30s - disconnecting WebSocket to save resources");
                provider.disconnect();
            }
        }, 30000);
    } else if (!document.hidden && provider && !provider.wsconnected) {
        log.info("Tab foregrounded - reconnecting WebSocket");
        provider.connect();
    }
};

const toggleTypeMenu = () => {
    showTypeMenu.value = !showTypeMenu.value;
    if (showTypeMenu.value) {
        showInsertMenu.value = false;
        showMoreMenu.value = false;
    }
};

const toggleInsertMenu = () => {
    showInsertMenu.value = !showInsertMenu.value;
    if (showInsertMenu.value) {
        showTypeMenu.value = false;
        showMoreMenu.value = false;
    }
};

const toggleMoreMenu = () => {
    showMoreMenu.value = !showMoreMenu.value;
    if (showMoreMenu.value) {
        showTypeMenu.value = false;
        showInsertMenu.value = false;
    }
};

// Functions to handle toolbar actions
const setHeading = (level: number) => {
    if (!editorView) return;
    const attrs = { level };
    setBlockType(schema.nodes.heading, attrs)(
        editorView.state,
        editorView.dispatch,
    );
};

const toggleBold = () => {
    if (!editorView) return;
    toggleMark(schema.marks.strong)(editorView.state, editorView.dispatch);
};

const toggleItalic = () => {
    if (!editorView) return;
    toggleMark(schema.marks.em)(editorView.state, editorView.dispatch);
};

const toggleBlockquote = () => {
    if (!editorView) return;
    setBlockType(schema.nodes.blockquote, {})(
        editorView.state,
        editorView.dispatch,
    );
};

const toggleCodeBlock = () => {
    if (!editorView) return;

    const { state, dispatch } = editorView;
    const { $from } = state.selection;

    // Check if already in a code block
    if ($from.parent.type === schema.nodes.code_block) {
        // Convert back to paragraph
        setBlockType(schema.nodes.paragraph, {})(state, dispatch);
    } else {
        // Ask for language
        const language = prompt(
            "Enter language for syntax highlighting (optional):",
            "",
        );
        const attrs = language ? { language } : {};
        setBlockType(schema.nodes.code_block, attrs)(state, dispatch);
    }

    editorView.focus();
};

const setParagraph = () => {
    if (!editorView) return;
    setBlockType(schema.nodes.paragraph, {})(
        editorView.state,
        editorView.dispatch,
    );
};

const toggleBulletList = () => {
    if (!editorView) return;
    wrapInList(schema.nodes.bullet_list)(editorView.state, editorView.dispatch);
};

const toggleOrderedList = () => {
    if (!editorView) return;
    wrapInList(schema.nodes.ordered_list)(
        editorView.state,
        editorView.dispatch,
    );
};

// Link tooltip handlers
const handleLinkApply = (url: string) => {
    if (!editorView) return;
    applyLink(url)(editorView.state, editorView.dispatch);
    editorView.focus();
};

const handleLinkRemove = () => {
    if (!editorView) return;
    removeLink()(editorView.state, editorView.dispatch);
    editorView.focus();
};

const handleLinkClose = () => {
    if (!editorView) return;
    hideLinkTooltip()(editorView.state, editorView.dispatch);
    editorView.focus();
};

const handleLinkOpen = (url: string) => {
    window.open(url, "_blank", "noopener,noreferrer");
};

// Handle link tooltip reposition request (on scroll)
const handleLinkReposition = () => {
    if (!editorView || !linkTooltipState.value.visible) return;

    // Recalculate position based on the link's current position in viewport
    const { from, to } = linkTooltipState.value;
    if (from === 0 && to === 0) return;

    try {
        const start = editorView.coordsAtPos(from);
        const end = editorView.coordsAtPos(to);

        linkTooltipState.value = {
            ...linkTooltipState.value,
            x: (start.left + end.left) / 2,
            y: end.bottom + 8,
        };
    } catch (e) {
        // Position might be invalid if doc changed, just hide
        hideLinkTooltip()(editorView.state, editorView.dispatch);
    }
};

// Show link tooltip (for toolbar button)
const insertLink = () => {
    if (!editorView) return;
    showLinkTooltip(true)(editorView.state, editorView.dispatch, editorView);
};

const undoEdit = () => {
    if (!editorView) return;
    undo(editorView.state);
};

const redoEdit = () => {
    if (!editorView) return;
    redo(editorView.state);
};

// Cleanup function
const cleanup = () => {
    if (reinitializeTimeout) {
        clearTimeout(reinitializeTimeout);
        reinitializeTimeout = null;
    }

    // CRITICAL: Clear visibility timeout to prevent disconnect after unmount
    if (visibilityTimeout) {
        clearTimeout(visibilityTimeout);
        visibilityTimeout = null;
    }

    // Clear connection timeout
    if (connectionTimeout) {
        clearTimeout(connectionTimeout);
        connectionTimeout = null;
    }

    // CRITICAL: Clean up in the correct order to prevent race conditions
    // 1. First disconnect the provider to stop new messages
    if (provider) {
        try {
            provider.disconnect();
        } catch (e) {
            log.error("Error disconnecting provider:", e);
        }
    }

    // 2. Destroy the editor view BEFORE awareness to prevent cursor updates on null editor
    if (editorView) {
        try {
            editorView.destroy();
            editorView = null;
        } catch (e) {
            log.error("Error destroying editor view:", e);
        }
    }

    // 3. Now safe to destroy provider and awareness
    if (provider) {
        try {
            // Remove all event listeners first to prevent callbacks during destruction
            // Use stored handler references for proper removal
            if (statusHandler) {
                provider.off("status", statusHandler);
                statusHandler = null;
            }
            if (statusReconnectHandler) {
                provider.off("status", statusReconnectHandler);
                statusReconnectHandler = null;
            }
            if (connectionErrorHandler) {
                provider.off("connection-error", connectionErrorHandler);
                connectionErrorHandler = null;
            }
            if (connectionCloseHandler) {
                provider.off("connection-close", connectionCloseHandler);
                connectionCloseHandler = null;
            }
            if (syncedHandler) {
                provider.off("synced", syncedHandler);
                syncedHandler = null;
            }

            // Remove awareness change handler
            if (provider.awareness && awarenessChangeHandler) {
                provider.awareness.off("change", awarenessChangeHandler);
                awarenessChangeHandler = null;
            }

            // Destroy awareness (this will no longer trigger editor updates)
            if (provider.awareness) {
                provider.awareness.destroy();
            }

            // Destroy provider
            provider.destroy();
            provider = null;
        } catch (e) {
            log.error("Error destroying provider:", e);
        }
    }

    // 4. Finally clean up Yjs document
    if (ydoc) {
        try {
            ydoc.destroy();
            ydoc = null;
        } catch (e) {
            log.error("Error destroying ydoc:", e);
        }
    }

    isConnected.value = false;
    isInitialized.value = false;
    connectionStatus.value = 'connecting';
    hasBeenConnected.value = false;
};

// Handle page unload to ensure clean disconnect
const handleBeforeUnload = (event: BeforeUnloadEvent) => {
    if (provider && provider.wsconnected) {
        // Attempt to disconnect cleanly
        provider.disconnect();
    }
};

// Watch for changes in the auth user and update awareness
watch(
    () => authStore.user,
    () => {
        if (provider && provider.awareness) {
            const currentState = provider.awareness.getLocalState() || {};
            provider.awareness.setLocalState({
                ...currentState,
                user: {
                    ...currentState?.user,
                    name: getUserDisplayName(),
                    avatar: authStore.user?.avatar_thumb || authStore.user?.avatar_url || undefined,
                },
            });
            log.debug(`Updated user name to: ${getUserDisplayName()}`);
        }
    },
);

// Add method to update editor state
const updateState = (newState: EditorState) => {
    if (editorView) {
        editorView.updateState(newState);
    }
};

// Helper function to log environment variables for debugging
const logEnvironmentInfo = () => {
    log.info("Environment info:");
    log.info(
        `- VITE_WS_SERVER_URL: ${import.meta.env.VITE_WS_SERVER_URL || "Not set"}`,
    );
    log.info(`- window.location.host: ${window.location.host}`);
    log.info(`- window.location.protocol: ${window.location.protocol}`);
    log.info(`- window.location.origin: ${window.location.origin}`);
};

// Add a function to inspect relative positions
const debugRelativePositions = () => {
    if (!editorView || !ydoc || !yXmlFragment || !provider) return;

    try {
        // Get current ProseMirror selection
        const selection = editorView.state.selection;

        // Log all current absolute positions
        log.debug("Current selection (absolute positions):", {
            anchor: selection.anchor,
            head: selection.head,
            from: selection.from,
            to: selection.to,
        });

        // Create relative positions from current selection
        const relAnchor = Y.createRelativePositionFromTypeIndex(
            yXmlFragment,
            selection.anchor,
        );
        const relHead = Y.createRelativePositionFromTypeIndex(
            yXmlFragment,
            selection.head,
        );

        // Log the relative positions as JSON for inspection
        log.debug("Relative positions:", {
            anchor: JSON.stringify(relAnchor),
            head: JSON.stringify(relHead),
        });

        // Try to convert back to absolute positions
        const absAnchor = Y.createAbsolutePositionFromRelativePosition(
            relAnchor,
            ydoc,
        );
        const absHead = Y.createAbsolutePositionFromRelativePosition(
            relHead,
            ydoc,
        );

        log.debug("Converted back to absolute positions:", {
            anchor: absAnchor ? absAnchor.index : "conversion failed",
            head: absHead ? absHead.index : "conversion failed",
        });

        // Check what's in the awareness states
        const states = provider.awareness.getStates();
        log.debug(`Awareness states (${states.size} clients):`);

        states.forEach((state, clientId) => {
            if (provider) {
                const isLocal = clientId === provider.awareness.clientID;
                if (state.cursor) {
                    log.debug(
                        `Client ${clientId}${isLocal ? " (local)" : ""}:`,
                        {
                            anchor: state.cursor.anchor,
                            head: state.cursor.head,
                            // If these positions are actually stored as relative positions in awareness,
                            // inspect them directly
                            anchorRelative:
                                typeof state.cursor.anchorRelative === "object"
                                    ? JSON.stringify(
                                          state.cursor.anchorRelative,
                                      )
                                    : "not available",
                            headRelative:
                                typeof state.cursor.headRelative === "object"
                                    ? JSON.stringify(state.cursor.headRelative)
                                    : "not available",
                        },
                    );
                }
            }
        });
    } catch (error) {
        log.error("Error in debugRelativePositions:", error);
    }
};

// Diagnostic function to help troubleshoot disconnection issues
const diagnoseConnectionIssue = () => {
    log.info("=== WebSocket Connection Diagnostics ===");

    // Environment configuration - derive WebSocket URL from API URL
    const apiUrl = import.meta.env.VITE_API_URL || '/api';
    let baseWsUrl = import.meta.env.VITE_WS_SERVER_URL;

    if (!baseWsUrl) {
        if (apiUrl.startsWith('/')) {
            const wsProtocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
            baseWsUrl = `${wsProtocol}//${window.location.host}${apiUrl}/collaboration/ws`;
        } else {
            baseWsUrl = apiUrl.replace(/^http/, 'ws') + '/collaboration/ws';
        }
    }

    log.info("Environment Configuration:", {
        nodeEnv: import.meta.env.NODE_ENV,
        mode: import.meta.env.MODE,
        apiUrl: apiUrl,
        wsServerUrl:
            import.meta.env.VITE_WS_SERVER_URL ||
            "Not set (derived from API URL)",
        computedWsUrl: baseWsUrl,
        windowLocation: {
            hostname: window.location.hostname,
            host: window.location.host,
            port: window.location.port,
            protocol: window.location.protocol,
            href: window.location.href,
        },
    });

    // Get auth store first
    const authStore = useAuthStore();

    // Authentication status
    const token = localStorage.getItem("token");
    log.info("Authentication Status:", {
        hasToken: !!token,
        tokenLength: token?.length || 0,
        tokenPrefix: token?.substring(0, 20) + "..." || "No token",
        userLoggedIn: !!authStore.user,
        userName: authStore.user?.name || "Not logged in",
        userUuid: authStore.user?.uuid || "No UUID",
    });

    // Network status
    log.info("Network Status:", {
        online: navigator.onLine,
        connection: (navigator as any).connection
            ? {
                  effectiveType: (navigator as any).connection.effectiveType,
                  downlink: (navigator as any).connection.downlink,
                  rtt: (navigator as any).connection.rtt,
              }
            : "Connection API not available",
    });

    // Document and provider status
    log.info("Collaboration Status:", {
        docId: props.docId,
        hasYdoc: !!ydoc,
        hasProvider: !!provider,
        providerConnected: provider?.wsconnected || false,
        providerConnecting: provider?.wsconnecting || false,
        hasEditorView: !!editorView,
        connectedUsers: connectedUsers.value.length,
    });

    // Troubleshooting suggestions
    log.info("=== Troubleshooting Suggestions ===");
    if (!authStore.isAuthenticated) {
        log.error("❌ Not authenticated - Please log in again");
    }
    if (!navigator.onLine) {
        log.error(
            "❌ Browser reports offline status - Check internet connection",
        );
    }
    if (
        import.meta.env.NODE_ENV === "development" &&
        !import.meta.env.VITE_WS_SERVER_URL
    ) {
        log.warn(
            "⚠️  VITE_WS_SERVER_URL not set - Using auto-detection which may not work in all environments",
        );
    }

    log.info("=== End Diagnostics ===");
};

// Debug object type for window.example
interface EditorDebugInfo {
    provider: WebsocketProvider | null;
    ydoc: Y.Doc | null;
    yXmlFragment: Y.XmlFragment | null;
    editorView: EditorView | null;
    diagnoseConnection: () => void;
}

// Add window debug methods
window.example = undefined; // Initialize with undefined until editor is created

// Update the global interface
declare global {
    interface Window {
        example?: EditorDebugInfo;
    }
}

onMounted(() => {
    initEditor();
    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("keydown", handleKeydown);
    window.addEventListener("beforeunload", handleBeforeUnload);

    // Set 10-second timeout for initial connection
    // If not connected within 10 seconds, show "Disconnected" instead of "Connecting"
    connectionTimeout = setTimeout(() => {
        if (connectionStatus.value === 'connecting') {
            connectionStatus.value = 'disconnected';
            log.warn('Connection timeout - failed to connect within 10 seconds');
        }
    }, 10000);

    // Add network status monitoring with stored handler references for proper cleanup
    onlineHandler = () => {
        log.info("Network came back online - websocket may reconnect automatically");
    };
    offlineHandler = () => {
        log.warn("Network went offline - websocket connection will be lost");
    };
    window.addEventListener("online", onlineHandler);
    window.addEventListener("offline", offlineHandler);

    // Add visibility change listener (handler defined at top level)
    document.addEventListener("visibilitychange", handleVisibilityChange);
});

onBeforeUnmount(() => {
    cleanup();
    document.removeEventListener("mousedown", handleClickOutside);
    document.removeEventListener("keydown", handleKeydown);
    window.removeEventListener("beforeunload", handleBeforeUnload);

    // Remove network status monitoring using stored handler references
    if (onlineHandler) {
        window.removeEventListener("online", onlineHandler);
        onlineHandler = null;
    }
    if (offlineHandler) {
        window.removeEventListener("offline", offlineHandler);
        offlineHandler = null;
    }

    // Clear visibility change debounce timeout
    if (visibilityTimeout) {
        clearTimeout(visibilityTimeout);
        visibilityTimeout = null;
    }

    // Clear mention search timer
    if (mentionSearchTimer) {
        clearTimeout(mentionSearchTimer);
        mentionSearchTimer = null;
    }

    // Remove visibility change listener
    document.removeEventListener("visibilitychange", handleVisibilityChange);
});

// Store original state when viewing revisions
let originalYXmlFragment: Y.XmlFragment | null = null;
let originalEditorState: EditorState | null = null;

// Revision viewing methods
function viewSnapshot(snapshotData: { snapshot: string; prevSnapshot: string; revision_number: number; yjs_document_content: string }) {
    if (!editorView || !ydoc || !yXmlFragment) {
        log.error("Cannot view snapshot: editor not initialized");
        return;
    }

    try {
        log.info(`Viewing revision ${snapshotData.revision_number}`);

        // Store the original state for restoring later
        if (!isViewingRevision.value) {
            originalYXmlFragment = yXmlFragment;
            originalEditorState = editorView.state;
        }

        // Decode the full document content for this revision
        log.info(`Base64 yjs_document_content length: ${snapshotData.yjs_document_content.length}`);
        const documentBytes = Uint8Array.from(atob(snapshotData.yjs_document_content), c => c.charCodeAt(0));
        log.info(`Decoded bytes length: ${documentBytes.length}`);
        log.info(`First 20 bytes: ${Array.from(documentBytes.slice(0, 20))}`);

        // Create a temporary Yjs document for viewing this revision
        // Disable GC to ensure all historical data is preserved
        const tempDoc = new Y.Doc({ gc: false });

        // Apply the revision's content to the temporary document FIRST
        log.info(`Applying update to temp doc...`);
        try {
            Y.applyUpdate(tempDoc, documentBytes);
            log.info(`Update applied successfully.`);
        } catch (err) {
            log.error(`Error applying update:`, err);
            throw err;
        }

        // NOW get the fragment after the update has been applied
        const tempFragment = tempDoc.getXmlFragment("prosemirror");
        log.info(`Got fragment after update. Children: ${tempFragment.length}`);

        // Debug: Log the Yjs fragment content
        log.info(`Temp doc state after applying update: ${tempDoc.store.clients.size} clients`);
        log.info(`Temp fragment children: ${tempFragment.length}`);
        log.info(`Temp fragment content: ${tempFragment.toString()}`);

        // Create a read-only ProseMirror state from this revision
        const { doc } = initProseMirrorDoc(tempFragment, schema);

        // Debug: Log the ProseMirror doc content
        log.info(`ProseMirror doc from revision: ${doc.textContent}`);

        // Create a read-only state with the revision content
        const readOnlyState = EditorState.create({
            doc,
            schema,
            plugins: [
                // Minimal plugins for read-only viewing
                keymap(baseKeymap),
                dropCursor(),
                twemojiPlugin,
            ],
        });

        // Update the editor view to show this read-only state
        editorView.updateState(readOnlyState);

        // Mark as viewing revision
        isViewingRevision.value = true;
        currentRevisionNumber.value = snapshotData.revision_number;

        log.info(`Successfully loaded revision ${snapshotData.revision_number} (read-only view)`);
    } catch (error) {
        log.error("Failed to view snapshot:", error);
        // If viewing fails, make sure to clear the viewing state
        isViewingRevision.value = false;
        currentRevisionNumber.value = null;
        throw error;
    }
}

function exitRevisionView() {
    if (!editorView || !originalEditorState || !originalYXmlFragment) {
        log.error("Cannot exit revision view: no original state stored");
        return;
    }

    try {
        log.info("Exiting revision view, returning to live document");

        // Restore the original editor state (connected to live Yjs doc)
        editorView.updateState(originalEditorState);

        // Clear stored state
        originalYXmlFragment = null;
        originalEditorState = null;

        // Mark as no longer viewing revision
        isViewingRevision.value = false;
        currentRevisionNumber.value = null;

        log.info("Successfully returned to live editing");
    } catch (error) {
        log.error("Failed to exit revision view:", error);
        throw error;
    }
}

// Handle revision selection from RevisionHistory component
const handleRevisionSelect = (revisionNumber: number | null) => {
    if (revisionNumber === null) {
        // User exited revision view
        log.info("Exiting revision view");
        // TODO: Reload current document state
    } else {
        // User selected a revision to view
        log.info(`User selected revision ${revisionNumber}`);
        // TODO: Load and display the revision (read-only mode)
    }
};

// Handle revision restoration
const handleRevisionRestored = (revisionNumber: number) => {
    log.info(`Revision ${revisionNumber} restored successfully`);
    showRevisionHistory.value = false;
    // The backend broadcast will update all clients automatically
};

// Expose methods and state for parent components
defineExpose({
    viewSnapshot,
    exitRevisionView,
    isViewingRevision,
    currentRevisionNumber
});
</script>

<template>
    <div class="collaborative-editor">
        <!-- Toolbar -->
        <div class="toolbar">
            <!-- Type Dropdown -->
            <div class="relative">
                <button
                    ref="typeButtonRef"
                    @click="toggleTypeMenu"
                    class="toolbar-button"
                    aria-haspopup="true"
                    :aria-expanded="showTypeMenu"
                    title="Text Style"
                >
                    <svg
                        xmlns="http://www.w3.org/2000/svg"
                        width="16"
                        height="16"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <path d="M4 7V4h16v3"></path>
                        <path d="M9 20h6"></path>
                        <path d="M12 4v16"></path>
                    </svg>
                </button>

                <!-- Type Menu Dropdown -->
                <Teleport to="body">
                    <div
                        v-if="showTypeMenu"
                        ref="typeMenuRef"
                        class="dropdown-menu-fixed"
                        :class="{ 'open-up': typeMenuPosition.openDirection === 'up' }"
                        :style="{
                            top: typeMenuPosition.openDirection === 'up' ? 'auto' : `${typeMenuPosition.top}px`,
                            bottom: typeMenuPosition.openDirection === 'up' ? `${typeMenuPosition.bottom}px` : 'auto',
                            left: `${typeMenuPosition.left}px`,
                            maxWidth: typeMenuPosition.maxWidth ? `${typeMenuPosition.maxWidth}px` : undefined
                        }"
                        role="menu"
                        tabindex="-1"
                    >
                        <button
                            @click="
                                setParagraph();
                                showTypeMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            Plain
                        </button>
                        <button
                            @click="
                                setHeading(1);
                                showTypeMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            Heading 1
                        </button>
                        <button
                            @click="
                                setHeading(2);
                                showTypeMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            Heading 2
                        </button>
                        <button
                            @click="
                                setHeading(3);
                                showTypeMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            Heading 3
                        </button>
                        <button
                            @click="
                                toggleBlockquote();
                                showTypeMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            Blockquote
                        </button>
                        <button
                            @click="
                                toggleCodeBlock();
                                showTypeMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            Code Block
                        </button>
                    </div>
                </Teleport>
            </div>

            <div class="toolbar-divider"></div>

            <!-- Formatting Buttons -->
            <button @click="toggleBold" class="toolbar-button" title="Bold">
                <svg
                    xmlns="http://www.w3.org/2000/svg"
                    width="16"
                    height="16"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                >
                    <path d="M6 4h8a4 4 0 0 1 4 4 4 4 0 0 1-4 4H6z"></path>
                    <path d="M6 12h9a4 4 0 0 1 4 4 4 4 0 0 1-4 4H6z"></path>
                </svg>
            </button>
            <button @click="toggleItalic" class="toolbar-button" title="Italic">
                <svg
                    xmlns="http://www.w3.org/2000/svg"
                    width="16"
                    height="16"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                >
                    <line x1="19" y1="4" x2="10" y2="4"></line>
                    <line x1="14" y1="20" x2="5" y2="20"></line>
                    <line x1="15" y1="4" x2="9" y2="20"></line>
                </svg>
            </button>

            <div class="toolbar-divider"></div>

            <!-- List buttons -->
            <button
                @click="toggleBulletList"
                class="toolbar-button"
                title="Bullet List"
            >
                <svg
                    xmlns="http://www.w3.org/2000/svg"
                    width="16"
                    height="16"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                >
                    <line x1="8" y1="6" x2="21" y2="6"></line>
                    <line x1="8" y1="12" x2="21" y2="12"></line>
                    <line x1="8" y1="18" x2="21" y2="18"></line>
                    <circle cx="3" cy="6" r="1"></circle>
                    <circle cx="3" cy="12" r="1"></circle>
                    <circle cx="3" cy="18" r="1"></circle>
                </svg>
            </button>
            <button
                @click="toggleOrderedList"
                class="toolbar-button"
                title="Numbered List"
            >
                <svg
                    xmlns="http://www.w3.org/2000/svg"
                    width="16"
                    height="16"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                >
                    <line x1="10" y1="6" x2="21" y2="6"></line>
                    <line x1="10" y1="12" x2="21" y2="12"></line>
                    <line x1="10" y1="18" x2="21" y2="18"></line>
                    <path d="M4 6h1v4"></path>
                    <path d="M4 10h2"></path>
                    <path d="M6 18H4c0-1 2-2 2-3s-1-1.5-2-1"></path>
                </svg>
            </button>

            <div class="toolbar-divider"></div>

            <!-- Insert Dropdown Menu with expanded options -->
            <div class="relative">
                <button
                    ref="insertButtonRef"
                    @click="toggleInsertMenu"
                    class="toolbar-button"
                    aria-haspopup="true"
                    :aria-expanded="showInsertMenu"
                    title="Insert"
                >
                    <svg
                        xmlns="http://www.w3.org/2000/svg"
                        width="16"
                        height="16"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <line x1="12" y1="5" x2="12" y2="19"></line>
                        <line x1="5" y1="12" x2="19" y2="12"></line>
                    </svg>
                </button>

                <!-- Insert Menu Dropdown -->
                <Teleport to="body">
                    <div
                        v-if="showInsertMenu"
                        ref="insertMenuRef"
                        class="dropdown-menu-fixed"
                        :class="{ 'open-up': insertMenuPosition.openDirection === 'up' }"
                        :style="{
                            top: insertMenuPosition.openDirection === 'up' ? 'auto' : `${insertMenuPosition.top}px`,
                            bottom: insertMenuPosition.openDirection === 'up' ? `${insertMenuPosition.bottom}px` : 'auto',
                            left: `${insertMenuPosition.left}px`,
                            maxWidth: insertMenuPosition.maxWidth ? `${insertMenuPosition.maxWidth}px` : undefined
                        }"
                        role="menu"
                        tabindex="-1"
                    >
                        <button
                            @click="
                                toggleBulletList();
                                showInsertMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            Bullet List
                        </button>
                        <button
                            @click="
                                toggleOrderedList();
                                showInsertMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            Numbered List
                        </button>
                        <button
                            @click="
                                toggleBlockquote();
                                showInsertMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            Blockquote
                        </button>
                        <button
                            @click="
                                toggleCodeBlock();
                                showInsertMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            Code Block
                        </button>
                        <button
                            @click="
                                insertLink();
                                showInsertMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            Link
                        </button>
                    </div>
                </Teleport>
            </div>

            <div class="toolbar-divider"></div>

            <!-- Undo/Redo Buttons -->
            <button @click="undoEdit" class="toolbar-button" title="Undo">
                <svg
                    xmlns="http://www.w3.org/2000/svg"
                    width="16"
                    height="16"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                >
                    <path d="M3 7v6h6"></path>
                    <path d="M21 17a9 9 0 0 0-9-9 9 9 0 0 0-6 2.3L3 13"></path>
                </svg>
            </button>
            <button @click="redoEdit" class="toolbar-button" title="Redo">
                <svg
                    xmlns="http://www.w3.org/2000/svg"
                    width="16"
                    height="16"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                >
                    <path d="M21 7v6h-6"></path>
                    <path d="M3 17a9 9 0 0 1 9-9 9 9 0 0 1 6 2.3L21 13"></path>
                </svg>
            </button>

            <!-- Revision History Button -->
            <button
                v-if="!hideRevisionHistory"
                @click="toggleRevisionHistory"
                class="toolbar-button"
                :class="{ 'toolbar-button-active': showRevisionHistory }"
                title="Revision History"
            >
                <svg
                    xmlns="http://www.w3.org/2000/svg"
                    width="16"
                    height="16"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                >
                    <circle cx="12" cy="12" r="10"></circle>
                    <polyline points="12 6 12 12 16 14"></polyline>
                </svg>
            </button>

            <!-- Spacer to push connection controls to right -->
            <div class="flex-grow"></div>

            <!-- Connected users -->
            <div
                v-if="connectedUsers.length > 0"
                class="flex items-center gap-1 mr-2"
            >
                <div class="text-xs text-tertiary mr-1">Editing with:</div>
                <div class="flex">
                    <div
                        v-for="(connectedUser, index) in connectedUsers"
                        :key="connectedUser.id"
                        class="flex items-center"
                        :style="{ marginLeft: index > 0 ? '-8px' : '0' }"
                        :title="
                            connectedUser.user.name +
                            (connectedUser.user.uuid
                                ? ' (UUID: ' + connectedUser.user.uuid + ')'
                                : '')
                        "
                        @click="
                            () => {
                                console.log('User data:', connectedUser.user);
                            }
                        "
                    >
                        <UserAvatar
                            :name="
                                connectedUser.user.uuid ||
                                connectedUser.user.name
                            "
                            :userName="connectedUser.user.name"
                            :avatar="connectedUser.user.avatar"
                            :showName="false"
                            size="xs"
                            :clickable="!!connectedUser.user.uuid"
                        />
                    </div>
                </div>
            </div>

            <!-- Connection status indicator - shown when connecting or disconnected -->
            <div v-if="connectionStatus === 'connecting'" class="connection-status-connecting">
                Connecting...
            </div>
            <div v-else-if="connectionStatus === 'disconnected'" class="connection-status-disconnected">
                Disconnected
            </div>
        </div>

        <!-- Editor content with click handler -->
        <div ref="editorWrapper" class="editor-wrapper">
            <div
                id="editor"
                ref="editorElement"
                @click="focusEditor"
                class="editor-container"
            ></div>

        </div>

        <!-- Mention Dropdown (teleported to body for proper positioning) -->
        <Teleport to="body">
            <Transition
                enter-active-class="transition ease-out duration-100"
                enter-from-class="transform opacity-0 scale-95"
                enter-to-class="transform opacity-100 scale-100"
                leave-active-class="transition ease-in duration-75"
                leave-from-class="transform opacity-100 scale-100"
                leave-to-class="transform opacity-0 scale-95"
            >
                <div
                    v-if="mentionState.active"
                    ref="mentionDropdownRef"
                    class="mention-dropdown"
                    :style="mentionDropdownStyle"
                >
                    <!-- Search indicator -->
                    <div v-if="mentionState.query" class="px-3 py-2 text-xs text-tertiary border-b border-default bg-surface-alt">
                        Searching for "<span class="text-primary font-medium">{{ mentionState.query }}</span>"
                    </div>

                    <!-- Loading -->
                    <div v-if="isMentionSearching" class="px-3 py-4 flex items-center justify-center">
                        <div class="animate-spin rounded-full h-4 w-4 border-2 border-accent border-t-transparent"></div>
                    </div>

                    <!-- User list -->
                    <div v-else-if="mentionUsers.length > 0" class="max-h-48 overflow-y-auto">
                        <button
                            v-for="(user, index) in mentionUsers"
                            :key="user.uuid"
                            type="button"
                            @click="selectMentionUser(user)"
                            @mouseenter="mentionSelectedIndex = index"
                            class="w-full px-3 py-2 flex items-center gap-3 text-left hover:bg-surface-alt transition-colors"
                            :class="{ 'bg-surface-alt selected': index === mentionSelectedIndex }"
                        >
                            <UserAvatar
                                :name="user.name"
                                :avatar="user.avatar_thumb || user.avatar_url"
                                size="sm"
                                :showName="false"
                            />
                            <div class="flex-1 min-w-0">
                                <p class="text-sm font-medium text-primary truncate">{{ user.name }}</p>
                                <p v-if="user.email" class="text-xs text-tertiary truncate">{{ user.email }}</p>
                            </div>
                        </button>
                    </div>

                    <!-- No results -->
                    <div v-else class="px-3 py-4 text-center text-sm text-tertiary">
                        No users found
                    </div>

                    <!-- Hint -->
                    <div class="px-3 py-2 text-xs text-tertiary border-t border-default bg-surface-alt flex items-center gap-4">
                        <span><kbd class="px-1 py-0.5 bg-surface rounded text-xs">↑↓</kbd> Navigate</span>
                        <span><kbd class="px-1 py-0.5 bg-surface rounded text-xs">Enter</kbd> Select</span>
                        <span><kbd class="px-1 py-0.5 bg-surface rounded text-xs">Esc</kbd> Close</span>
                    </div>
                </div>
            </Transition>
        </Teleport>

        <!-- Link Tooltip -->
        <LinkTooltip
            :visible="linkTooltipState.visible"
            :url="linkTooltipState.url"
            :x="linkTooltipState.x"
            :y="linkTooltipState.y"
            :is-editing="linkTooltipState.isEditing"
            @apply="handleLinkApply"
            @remove="handleLinkRemove"
            @close="handleLinkClose"
            @open-link="handleLinkOpen"
            @request-reposition="handleLinkReposition"
        />

        <!-- Revision History Sidebar -->
        <transition name="slide-left">
            <RevisionHistory
                v-if="showRevisionHistory"
                :ticket-id="ticketId"
                @close="showRevisionHistory = false"
                @select-revision="handleRevisionSelect"
                @restored="handleRevisionRestored"
            />
        </transition>
    </div>
</template>

<style>
.collaborative-editor {
    display: flex;
    flex-direction: column;
    border-radius: 0 0 0.75rem 0.75rem;
    overflow: hidden;
    background-color: var(--color-surface);
    height: 100%;
    width: 100%;
    position: relative;
}

.toolbar {
    display: flex;
    padding: 0.5rem;
    background-color: var(--color-surface);
    border-bottom: 1px solid var(--color-default);
    flex-wrap: wrap;
    gap: 0.25rem;
    align-items: center;
}

.toolbar-button {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0.25rem 0.5rem;
    background-color: var(--color-surface);
    border: none;
    border-radius: 0.375rem; /* rounded-md */
    color: var(--color-secondary);
    cursor: pointer;
    font-size: 0.875rem;
    transition: all 0.2s;
}

.toolbar-button:hover {
    background-color: var(--color-surface-hover);
    color: var(--color-primary);
}

.toolbar-button.active {
    color: var(--color-accent);
}

.toolbar-divider {
    width: 1px;
    height: 1.5rem;
    background-color: var(--color-default);
    margin: 0 0.5rem;
}

.dropdown-menu {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 0.25rem;
    width: 12rem;
    background-color: var(--color-surface);
    border: 1px solid var(--color-default);
    border-radius: 0.5rem; /* rounded-lg */
    box-shadow:
        0 10px 15px -3px rgba(0, 0, 0, 0.1),
        0 4px 6px -2px rgba(0, 0, 0, 0.05);
    z-index: 50;
    overflow: hidden;
}

/* Fixed positioned dropdown for Teleport usage (viewport-aware) */
.dropdown-menu-fixed {
    position: fixed;
    width: 12rem;
    background-color: var(--color-surface);
    border: 1px solid var(--color-default);
    border-radius: 0.5rem;
    box-shadow:
        0 10px 15px -3px rgba(0, 0, 0, 0.1),
        0 4px 6px -2px rgba(0, 0, 0, 0.05);
    z-index: 9999;
    overflow: hidden;
    transform-origin: top left;
}

.dropdown-menu-fixed.open-up {
    transform-origin: bottom left;
}

.dropdown-item {
    display: block;
    width: 100%;
    padding: 0.5rem 1rem;
    text-align: left;
    font-size: 0.875rem;
    color: var(--color-primary);
    background-color: transparent;
    border: none;
    cursor: pointer;
    transition: background-color 0.2s;
}

.dropdown-item:hover {
    background-color: var(--color-surface-hover);
    color: var(--color-primary);
}

.connection-status-connecting {
    font-size: 0.75rem;
    font-weight: 500;
    color: var(--color-status-warning, #f59e0b);
    padding: 0.25rem 0.625rem;
    border-radius: 0.375rem;
    background-color: var(--color-status-warning-bg, rgba(245, 158, 11, 0.15));
    border: 1px solid var(--color-status-warning-border, rgba(245, 158, 11, 0.3));
}

.connection-status-disconnected {
    font-size: 0.75rem;
    font-weight: 500;
    color: var(--color-status-error);
    padding: 0.25rem 0.625rem;
    border-radius: 0.375rem;
    background-color: var(--color-status-error-bg, rgba(239, 68, 68, 0.15));
    border: 1px solid var(--color-status-error-border, rgba(239, 68, 68, 0.3));
}

.editor-wrapper {
    position: relative;
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
}

.mention-dropdown {
    z-index: 9999;
    min-width: 250px;
    max-width: 350px;
    background-color: var(--color-surface);
    border: 1px solid var(--color-default);
    border-radius: 0.5rem;
    box-shadow: 0 10px 15px -3px rgba(0, 0, 0, 0.1), 0 4px 6px -2px rgba(0, 0, 0, 0.05);
    overflow: hidden;
}

/* Mention typing highlight */
.ProseMirror .mention-typing {
    background-color: color-mix(in srgb, var(--color-accent) 15%, transparent);
    border-radius: 0.25rem;
}

/* Mention chip styles */
.ProseMirror .mention-chip {
    display: inline-flex;
    align-items: center;
    gap: 0.1875rem;
    padding: 0.0625em 0.375em 0.0625em 0.1875em;
    margin: 0 0.0625em;
    border-radius: 9999px;
    background-color: color-mix(in srgb, var(--color-accent) 12%, transparent);
    color: var(--color-accent);
    font-weight: 500;
    font-size: inherit;
    line-height: 1;
    vertical-align: text-bottom;
    cursor: pointer;
    user-select: none;
    transition: background-color 0.15s ease;
}

.ProseMirror .mention-chip:hover {
    background-color: color-mix(in srgb, var(--color-accent) 20%, transparent);
}

.ProseMirror .mention-avatar {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1em;
    height: 1em;
    border-radius: 9999px;
    overflow: hidden;
    flex-shrink: 0;
}

.ProseMirror .mention-avatar-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
}

.ProseMirror .mention-avatar-fallback {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: white;
    font-size: 0.5em;
    font-weight: 600;
}

.ProseMirror .mention-name {
    white-space: nowrap;
}

.editor-container {
    position: relative;
    background-color: var(--color-surface);
    border-radius: 0 0 0.5rem 0.5rem;
    color: var(--color-primary);
    font-family:
        ui-sans-serif,
        system-ui,
        -apple-system,
        BlinkMacSystemFont,
        "Segoe UI",
        Roboto,
        "Helvetica Neue",
        Arial,
        sans-serif;
    font-size: 1rem;
    line-height: 1.5;
    min-height: 200px;
    height: auto;
    overflow: visible;
    width: 100%;
}

.ProseMirror {
    outline: none;
    padding: 1rem;
    min-height: 200px;
    height: auto;
    overflow: visible;
    width: 100%;
    /* Ensure cursor is always visible, even in empty editor */
    min-height: 1.5em;
    /* Prevent iOS Safari from zooming when focusing on the editor */
    /* Font size must be at least 16px to prevent auto-zoom on iOS */
    font-size: max(1rem, 16px);
    /* Prevent double-tap zoom on touch devices */
    touch-action: manipulation;
}

/* Force cursor visibility in Chrome for empty contenteditable */
.ProseMirror:empty:before {
    content: "";
    display: inline-block;
    width: 0;
}

/* Always show cursor, never hide it */
.ProseMirror {
    caret-color: currentColor !important;
}

/* Fix for cursor visibility when first paragraph is empty - from Yjs demo */
.ProseMirror > .ProseMirror-yjs-cursor:first-child {
    margin-top: 16px;
}

.ProseMirror p:first-child,
.ProseMirror h1:first-child,
.ProseMirror h2:first-child,
.ProseMirror h3:first-child,
.ProseMirror h4:first-child,
.ProseMirror h5:first-child,
.ProseMirror h6:first-child {
    margin-top: 0;
}

/* Ensures the content doesn't overflow the container */
.editor-wrapper {
    height: auto;
    min-height: 200px;
    width: 100%;
    display: flex;
    flex-direction: column;
    overflow: visible;
}

/* Style for the editor container when active and there are users connected */
.collaboration-active {
    border: 1px solid var(--color-accent);
    border-radius: 0.5rem;
}

/* Ensure toolbar doesn't restrict editor content */
.editor-toolbar {
    position: sticky;
    top: 0;
    z-index: 10;
    background-color: var(--color-surface-alt);
    border-top-left-radius: 0.5rem;
    border-top-right-radius: 0.5rem;
    border-bottom: 1px solid var(--color-default);
    padding: 0.5rem;
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem;
}

.ProseMirror p {
    margin-top: 0.5rem;
    margin-bottom: 0.5rem;
    line-height: 1.6;
}

.ProseMirror h1 {
    font-size: 2rem;
    font-weight: 700;
    margin-top: 1rem;
    margin-bottom: 1rem;
    border-bottom: 1px solid var(--color-default);
    padding-bottom: 0.5rem;
    line-height: 1.2;
}

.ProseMirror h2 {
    font-size: 1.5rem;
    font-weight: 700;
    margin-top: 1.5rem;
    margin-bottom: 1rem;
    line-height: 1.3;
}

.ProseMirror h3 {
    font-size: 1.25rem;
    font-weight: 600;
    margin-top: 1.5rem;
    margin-bottom: 1rem;
    line-height: 1.4;
}

.ProseMirror blockquote {
    border-left: 3px solid var(--color-border-subtle);
    padding-left: 1rem;
    padding-right: 1rem;
    padding-top: 0.5rem;
    padding-bottom: 0.5rem;
    margin-left: 0;
    margin-right: 0;
    color: var(--color-secondary);
    margin-top: 1rem;
    margin-bottom: 1rem;
    background-color: var(--color-surface-alt);
    border-radius: 0.375rem;
}

.ProseMirror pre {
    background-color: var(--color-app);
    padding: 0.75rem;
    border-radius: 0.5rem; /* rounded-lg */
    overflow-x: auto;
    font-family:
        ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas,
        "Liberation Mono", "Courier New", monospace;
    margin-top: 1rem;
    margin-bottom: 1rem;
    border: 1px solid var(--color-subtle);
    position: relative;
}

/* Language indicator for code blocks */
.ProseMirror pre[data-language]::before {
    content: attr(data-language);
    position: absolute;
    top: 0;
    right: 0;
    padding: 0.25rem 0.5rem;
    background-color: var(--color-surface-alt);
    color: var(--color-secondary);
    font-size: 0.75rem;
    border-bottom-left-radius: 0.25rem;
    font-family:
        ui-sans-serif,
        system-ui,
        -apple-system,
        BlinkMacSystemFont,
        "Segoe UI",
        Roboto,
        "Helvetica Neue",
        Arial,
        sans-serif;
}

.ProseMirror pre code {
    background-color: transparent;
    padding: 0;
    border-radius: 0;
    color: var(--color-primary);
    display: block;
    white-space: pre-wrap;
    word-break: break-word;
}

/* Syntax highlighting using theme variables (via prosemirror-highlight + lowlight) */
.ProseMirror pre code .hljs-comment,
.ProseMirror pre code .hljs-quote {
    color: var(--color-syntax-comment, #6a737d);
    font-style: italic;
}

.ProseMirror pre code .hljs-keyword,
.ProseMirror pre code .hljs-selector-tag,
.ProseMirror pre code .hljs-meta {
    color: var(--color-syntax-keyword, #d73a49);
}

.ProseMirror pre code .hljs-string,
.ProseMirror pre code .hljs-attr,
.ProseMirror pre code .hljs-selector-attr,
.ProseMirror pre code .hljs-selector-pseudo {
    color: var(--color-syntax-string, #22863a);
}

.ProseMirror pre code .hljs-number,
.ProseMirror pre code .hljs-literal,
.ProseMirror pre code .hljs-symbol,
.ProseMirror pre code .hljs-bullet {
    color: var(--color-syntax-number, #005cc5);
}

.ProseMirror pre code .hljs-title,
.ProseMirror pre code .hljs-title.function_,
.ProseMirror pre code .hljs-section {
    color: var(--color-syntax-function, #6f42c1);
}

.ProseMirror pre code .hljs-variable,
.ProseMirror pre code .hljs-variable.language_,
.ProseMirror pre code .hljs-variable.constant_,
.ProseMirror pre code .hljs-params {
    color: var(--color-syntax-variable, #e36209);
}

.ProseMirror pre code .hljs-type,
.ProseMirror pre code .hljs-title.class_,
.ProseMirror pre code .hljs-built_in {
    color: var(--color-syntax-type, #22863a);
}

.ProseMirror pre code .hljs-operator,
.ProseMirror pre code .hljs-punctuation {
    color: var(--color-syntax-operator, #6a737d);
}

.ProseMirror pre code .hljs-property,
.ProseMirror pre code .hljs-attribute {
    color: var(--color-syntax-variable, #e36209);
}

.ProseMirror pre code .hljs-regexp {
    color: var(--color-syntax-string, #22863a);
}

.ProseMirror pre code .hljs-tag {
    color: var(--color-syntax-keyword, #d73a49);
}

.ProseMirror pre code .hljs-name {
    color: var(--color-syntax-type, #22863a);
}

.ProseMirror pre code .hljs-selector-id,
.ProseMirror pre code .hljs-selector-class {
    color: var(--color-syntax-function, #6f42c1);
}

.ProseMirror pre code .hljs-emphasis {
    font-style: italic;
}

.ProseMirror pre code .hljs-strong {
    font-weight: bold;
}

.ProseMirror pre code .hljs-link {
    text-decoration: underline;
}

.ProseMirror pre code .hljs-addition {
    color: var(--color-syntax-string, #22863a);
    background-color: rgba(34, 134, 58, 0.1);
}

.ProseMirror pre code .hljs-deletion {
    color: var(--color-syntax-keyword, #d73a49);
    background-color: rgba(215, 58, 73, 0.1);
}

.ProseMirror code {
    background-color: var(--color-surface);
    padding: 0.125rem 0.375rem;
    border-radius: 0.25rem;
    font-family:
        ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas,
        "Liberation Mono", "Courier New", monospace;
    color: var(--color-primary);
}

.ProseMirror ul,
.ProseMirror ol {
    padding-left: 1.5rem;
    margin-top: 1rem;
    margin-bottom: 1rem;
}

.ProseMirror li {
    margin-bottom: 0.5rem;
    line-height: 1.6;
}

/* Enhanced list styles */
.ProseMirror ul {
    list-style-type: disc;
    color: var(--color-primary);
}

.ProseMirror ul ul {
    list-style-type: circle;
}

.ProseMirror ul ul ul {
    list-style-type: square;
}

.ProseMirror ol {
    list-style-type: decimal;
    color: var(--color-primary);
}

.ProseMirror ol ol {
    list-style-type: lower-alpha;
}

.ProseMirror ol ol ol {
    list-style-type: lower-roman;
}

.ProseMirror li p {
    margin: 0.25rem 0;
}

.ProseMirror a {
    color: var(--color-accent);
    text-decoration: underline;
}

.ProseMirror a:hover {
    color: var(--color-accent-hover, var(--color-accent));
}

/* Ticket Drop Preview - shows ticket preview when dragging over editor */
.ProseMirror .ticket-drop-preview {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 12px;
    margin: 8px 0;
    background: var(--color-surface-alt);
    border: 2px dashed var(--color-accent);
    border-radius: 6px;
    font-size: 13px;
    line-height: 1.4;
    width: 100%;
    max-width: 100%;
    pointer-events: none;
    opacity: 0.85;
    animation: ticket-drop-preview-pulse 1.5s ease-in-out infinite;
    box-shadow: 0 0 12px rgba(var(--color-accent-rgb, 59, 130, 246), 0.3);
}

@keyframes ticket-drop-preview-pulse {
    0%, 100% {
        opacity: 0.75;
        box-shadow: 0 0 8px rgba(var(--color-accent-rgb, 59, 130, 246), 0.2);
    }
    50% {
        opacity: 0.95;
        box-shadow: 0 0 16px rgba(var(--color-accent-rgb, 59, 130, 246), 0.4);
    }
}

.ProseMirror .ticket-drop-preview-header {
    display: flex;
    align-items: center;
    gap: 8px;
}

.ProseMirror .ticket-drop-preview-id {
    font-weight: 600;
    color: var(--color-accent);
    font-family: var(--font-mono, monospace);
    font-size: 12px;
    flex-shrink: 0;
}

.ProseMirror .ticket-drop-preview-title {
    color: var(--color-text-primary, #fff);
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
}

.ProseMirror .ticket-drop-preview-meta {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    font-size: 11px;
}

.ProseMirror .ticket-drop-preview-person {
    color: var(--color-text-secondary, #aaa);
}

.ProseMirror .ticket-drop-preview-label {
    color: var(--color-text-tertiary, #666);
}

/* Status badges for drop preview */
.ProseMirror .ticket-drop-preview-status {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 4px;
    font-weight: 500;
    text-transform: capitalize;
}

.ProseMirror .ticket-drop-preview-status-open {
    background: var(--color-status-open-muted, rgba(59, 130, 246, 0.15));
    color: var(--color-status-open, #3b82f6);
}

.ProseMirror .ticket-drop-preview-status-in-progress {
    background: var(--color-status-in-progress-muted, rgba(245, 158, 11, 0.15));
    color: var(--color-status-in-progress, #f59e0b);
}

.ProseMirror .ticket-drop-preview-status-closed {
    background: var(--color-status-closed-muted, rgba(34, 197, 94, 0.15));
    color: var(--color-status-closed, #22c55e);
}

/* Priority badges for drop preview */
.ProseMirror .ticket-drop-preview-priority {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 4px;
    font-weight: 500;
}

.ProseMirror .ticket-drop-preview-priority-high {
    background: var(--color-priority-high-muted, rgba(239, 68, 68, 0.15));
    color: var(--color-priority-high, #ef4444);
}

.ProseMirror .ticket-drop-preview-priority-medium {
    background: var(--color-priority-medium-muted, rgba(245, 158, 11, 0.15));
    color: var(--color-priority-medium, #f59e0b);
}

.ProseMirror .ticket-drop-preview-priority-low {
    background: var(--color-priority-low-muted, rgba(34, 197, 94, 0.15));
    color: var(--color-priority-low, #22c55e);
}

/* Skeleton loading for drop preview fallback */
.ProseMirror .ticket-drop-preview-skeleton {
    display: inline-block;
    height: 14px;
    min-width: 80px;
    background: linear-gradient(
        90deg,
        var(--color-surface-hover) 25%,
        var(--color-surface-alt) 50%,
        var(--color-surface-hover) 75%
    );
    background-size: 200% 100%;
    animation: ticket-drop-skeleton-shimmer 1.5s ease-in-out infinite;
    border-radius: 4px;
}

@keyframes ticket-drop-skeleton-shimmer {
    0% {
        background-position: 200% 0;
    }
    100% {
        background-position: -200% 0;
    }
}

/* Ticket Link Card Styles - Full width card layout */
.ProseMirror .ticket-link-card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 12px;
    margin: 8px 0;
    background: var(--color-surface-alt);
    border: 1px solid var(--color-border-subtle);
    border-radius: 6px;
    font-size: 13px;
    line-height: 1.4;
    cursor: pointer;
    transition: all 0.15s ease;
    width: 100%;
    max-width: 100%;
}

.ProseMirror .ticket-link-card:hover {
    border-color: var(--color-accent);
    background: var(--color-surface-hover);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
}

.ProseMirror .ticket-link-loading {
    opacity: 0.7;
}

.ProseMirror .ticket-link-error {
    border-color: var(--color-status-error);
}

/* Header with ticket ID and title */
.ProseMirror .ticket-link-header {
    display: flex;
    align-items: center;
    gap: 8px;
}

.ProseMirror .ticket-link-id {
    font-weight: 600;
    color: var(--color-text-secondary, #888);
    font-family: var(--font-mono, monospace);
    font-size: 12px;
    flex-shrink: 0;
}

.ProseMirror .ticket-link-title {
    color: var(--color-text-primary, #fff);
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
}

/* Meta row with people and badges inline */
.ProseMirror .ticket-link-meta {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    font-size: 11px;
}

.ProseMirror .ticket-link-person {
    color: var(--color-text-secondary, #aaa);
}

.ProseMirror .ticket-link-label {
    color: var(--color-text-tertiary, #666);
}

/* Status badge */
.ProseMirror .ticket-link-status {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 4px;
    font-weight: 500;
    text-transform: capitalize;
}

.ProseMirror .ticket-link-status-open {
    background: var(--color-status-open-muted, rgba(59, 130, 246, 0.15));
    color: var(--color-status-open, #3b82f6);
}

.ProseMirror .ticket-link-status-in-progress {
    background: var(--color-status-in-progress-muted, rgba(245, 158, 11, 0.15));
    color: var(--color-status-in-progress, #f59e0b);
}

.ProseMirror .ticket-link-status-closed {
    background: var(--color-status-closed-muted, rgba(34, 197, 94, 0.15));
    color: var(--color-status-closed, #22c55e);
}

/* Priority badge */
.ProseMirror .ticket-link-priority {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 4px;
    font-weight: 500;
    text-transform: capitalize;
}

.ProseMirror .ticket-link-priority-high {
    background: var(--color-priority-high-muted, rgba(239, 68, 68, 0.15));
    color: var(--color-priority-high, #ef4444);
}

.ProseMirror .ticket-link-priority-medium {
    background: var(--color-priority-medium-muted, rgba(245, 158, 11, 0.15));
    color: var(--color-priority-medium, #f59e0b);
}

.ProseMirror .ticket-link-priority-low {
    background: var(--color-priority-low-muted, rgba(34, 197, 94, 0.15));
    color: var(--color-priority-low, #22c55e);
}

/* Colorblind mode indicator icons */
.ProseMirror .ticket-link-status .indicator-icon,
.ProseMirror .ticket-link-priority .indicator-icon {
    width: 10px;
    height: 10px;
    flex-shrink: 0;
}

/* Status indicator colors for colorblind mode */
.ProseMirror .text-status-open {
    color: var(--color-status-open, #3b82f6);
}

.ProseMirror .text-status-in-progress {
    color: var(--color-status-in-progress, #f59e0b);
}

.ProseMirror .text-status-closed {
    color: var(--color-status-closed, #22c55e);
}

/* Priority indicator colors for colorblind mode */
.ProseMirror .text-priority-low {
    color: var(--color-priority-low, #22c55e);
}

.ProseMirror .text-priority-medium {
    color: var(--color-priority-medium, #f59e0b);
}

.ProseMirror .text-priority-high {
    color: var(--color-priority-high, #ef4444);
}

/* Loading spinner */
.ProseMirror .ticket-link-loader {
    width: 12px;
    height: 12px;
    border: 2px solid var(--color-border-default, #333);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: ticket-link-spin 0.8s linear infinite;
}

@keyframes ticket-link-spin {
    to { transform: rotate(360deg); }
}

.ProseMirror strong {
    font-weight: 700;
    color: var(--color-primary);
}

.ProseMirror em {
    font-style: italic;
    color: var(--color-primary);
}

.ProseMirror .yRemoteSelection {
    position: absolute;
    border-left: 2px solid;
    border-right: 2px solid;
    pointer-events: none;
    opacity: 0.5;
    background-color: var(--color-accent-bg, rgba(59, 130, 246, 0.2));
}

.ProseMirror .yRemoteSelectionHead {
    position: absolute;
    height: 1.2em;
    width: 2px;
    pointer-events: none;
}

/* Flex spacer */
.flex-grow {
    flex-grow: 1;
}

/* This gives the remote user caret. The colors are automatically overwritten*/
.ProseMirror-yjs-cursor {
    position: relative;
    margin-left: -1px;
    margin-right: -1px;
    border-left: 1px solid orange;
    border-right: 1px solid orange;
    border-color: orange;
    word-break: normal;
    pointer-events: none;
    opacity: 1;
    height: 1.2em;
}

/* This renders the username above the caret */
.ProseMirror-yjs-cursor > div {
    position: absolute;
    top: -1.5em;
    left: -2px;
    font-size: 12px;
    background-color: currentColor;
    font-family: sans-serif;
    font-weight: normal;
    line-height: normal;
    user-select: none;
    color: white;
    padding: 1px 5px;
    white-space: nowrap;
    border-radius: 4px;
    max-width: 150px;
    overflow: hidden;
    text-overflow: ellipsis;
    z-index: 10;
}

/* Revision History Sidebar */
.collaborative-editor {
    position: relative;
}

/* Slide-left transition */
.slide-left-enter-active,
.slide-left-leave-active {
    transition: transform 0.3s ease;
}

.slide-left-enter-from {
    transform: translateX(100%);
}

.slide-left-leave-to {
    transform: translateX(100%);
}

/* Toolbar button active state */
.toolbar-button-active {
    background-color: var(--color-surface-alt);
}

/* Image upload placeholder styles */
.image-upload-placeholder {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.75rem 1rem;
    background-color: var(--color-surface-alt);
    border: 1px dashed var(--color-default);
    border-radius: 0.5rem;
    color: var(--color-secondary);
    font-size: 0.875rem;
    margin: 0.25rem 0;
}

.image-upload-spinner {
    width: 1rem;
    height: 1rem;
    border: 2px solid var(--color-default);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
}

@keyframes spin {
    to {
        transform: rotate(360deg);
    }
}

/* ============================================
   PRINT STYLES
   ============================================ */
@media print {
  /* Hide toolbar and interactive elements */
  .toolbar,
  .editor-toolbar,
  .connection-status-connecting,
  .connection-status-disconnected,
  .mention-dropdown,
  .dropdown-menu,
  .dropdown-menu-fixed {
    display: none !important;
  }

  /* Editor container - clean for print */
  .collaborative-editor {
    background: white !important;
    border: none !important;
  }

  .editor-container {
    background: white !important;
    border: none !important;
  }

  .ProseMirror {
    padding: 0 !important;
    border: none !important;
    background: transparent !important;
    min-height: auto !important;
  }

  /* Remove cursor indicators */
  .ProseMirror-yjs-cursor,
  .ProseMirror .yRemoteSelection,
  .ProseMirror .yRemoteSelectionHead {
    display: none !important;
  }

  /* Code blocks - visible border, light background */
  .ProseMirror pre {
    border: 1px solid #ccc !important;
    background: #f8f9fa !important;
    padding: 0.5rem !important;
    font-size: 9pt !important;
    page-break-inside: avoid !important;
    white-space: pre-wrap !important;
    word-break: break-word !important;
  }

  .ProseMirror pre code {
    background: transparent !important;
    color: #000 !important;
  }

  /* Syntax highlighting - print in grayscale */
  .ProseMirror pre code .hljs-comment,
  .ProseMirror pre code .hljs-quote {
    color: #666 !important;
  }

  .ProseMirror pre code .hljs-keyword,
  .ProseMirror pre code .hljs-string,
  .ProseMirror pre code .hljs-number,
  .ProseMirror pre code .hljs-title,
  .ProseMirror pre code .hljs-variable,
  .ProseMirror pre code .hljs-type {
    color: #333 !important;
  }

  /* Tables */
  .ProseMirror table {
    border-collapse: collapse !important;
    width: 100% !important;
  }

  .ProseMirror th,
  .ProseMirror td {
    border: 1px solid #ccc !important;
    padding: 0.25rem 0.5rem !important;
  }

  .ProseMirror th {
    background: #f3f4f6 !important;
  }

  /* Images - reasonable max size */
  .ProseMirror img {
    max-height: 4in !important;
    object-fit: contain !important;
    page-break-inside: avoid !important;
  }

  /* Blockquotes */
  .ProseMirror blockquote {
    border-left: 2px solid #666 !important;
    background: transparent !important;
    padding-left: 0.75rem !important;
    margin-left: 0 !important;
    color: #333 !important;
  }

  /* Inline code */
  .ProseMirror code {
    background: #f3f4f6 !important;
    border: 1px solid #e5e5e5 !important;
    color: #000 !important;
  }

  /* Links - show as regular text */
  .ProseMirror a {
    color: #000 !important;
    text-decoration: underline !important;
  }

  /* Ticket link cards - simplified for print */
  .ProseMirror .ticket-link-card {
    border: 1px solid #ccc !important;
    background: #fafafa !important;
    page-break-inside: avoid !important;
  }

  .ProseMirror .ticket-link-id,
  .ProseMirror .ticket-link-title,
  .ProseMirror .ticket-link-meta {
    color: #000 !important;
  }

  .ProseMirror .ticket-link-status,
  .ProseMirror .ticket-link-priority {
    background: transparent !important;
    border: 1px solid currentColor !important;
  }

  /* Hide loading states and drop previews */
  .ProseMirror .ticket-drop-preview,
  .ProseMirror .ticket-link-loading,
  .ProseMirror .ticket-link-loader,
  .ProseMirror .image-upload-placeholder {
    display: none !important;
  }

  /* Mention chips - simplified */
  .ProseMirror .mention-chip {
    background: transparent !important;
    color: #000 !important;
    border: 1px solid #ccc !important;
  }

  /* Headings */
  .ProseMirror h1,
  .ProseMirror h2,
  .ProseMirror h3,
  .ProseMirror h4 {
    color: #000 !important;
    page-break-after: avoid !important;
  }

  .ProseMirror h1 {
    border-bottom: 1px solid #ccc !important;
  }

  /* Lists */
  .ProseMirror ul,
  .ProseMirror ol {
    color: #000 !important;
  }
}
</style>
