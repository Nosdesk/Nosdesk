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
import { useFluent } from "fluent-vue";
import Spinner from "@/components/common/Spinner.vue";
import * as Y from "yjs";
import { PermanentUserData } from "yjs";
import { WebsocketProvider } from "y-websocket";
import { useCollabSessionStore, type ConnectionStatus } from "@/stores/collabSession";
import { SafePermanentUserData } from "@nosdesk/core/utils/safePermanentUserData";
import { getCollabWsUrl } from "@/utils/collabWsUrl";
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
import { createEmbeddedDocumentPlugin, setDocumentNavigationHandler } from "./editor/embeddedDocumentPlugin";
import DocumentPicker from "./editor/DocumentPicker.vue";
import apiClient from "@/services/apiConfig";
import {
    ySyncPlugin,
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
    exitCode,
    baseKeymap,
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
import { useUserMentionSearch } from "@/composables/useUserMentionSearch";

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

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

// Set up Vue Router navigation for ticket link cards
const router = useRouter();
setTicketNavigationHandler((ticketId: number) => {
    router.push(`/tickets/${ticketId}`);
});

setDocumentNavigationHandler((slug: string) => {
    // The documentation page route resolves by slug, so navigate by the
    // doc's slug (resolved from its uuid by the embed before calling).
    router.push(`/documentation/${slug}`);
});

// Document picker state
const showDocumentPicker = ref(false);

// Build an `embedded_document` node from a doc reference. Shared by the
// picker-driven insert and the promote-to-document replace so the node
// shape stays in one place.
const buildEmbeddedDocNode = (doc: { uuid: string; title: string }) => {
    if (!editorView) return null;
    const type = editorView.state.schema.nodes.embedded_document;
    if (!type) return null;
    return type.create({ documentUuid: doc.uuid, documentTitle: doc.title });
};

const insertEmbeddedDocument = (doc: { uuid: string; title: string }) => {
    showDocumentPicker.value = false;
    if (!editorView) return;

    const node = buildEmbeddedDocNode(doc);
    if (!node) return;

    const tr = editorView.state.tr.replaceSelectionWith(node);
    editorView.dispatch(tr);
    editorView.focus();

    // Sync embeddings after inserting
    syncEmbeddings();
};

// Replace the entire document body with a single embedded-document node.
// Used when promoting a ticket note into a standalone document: the
// note's content is cloned into the new doc server-side, then the note
// body becomes a transclusion of that doc (one source of truth).
const replaceAllWithEmbeddedDocument = (doc: { uuid: string; title: string }) => {
    if (!editorView) return;

    const node = buildEmbeddedDocNode(doc);
    if (!node) return;

    const { state } = editorView;
    const tr = state.tr.replaceWith(0, state.doc.content.size, node);
    editorView.dispatch(tr);

    syncEmbeddings();
};

// Extract embedded document UUIDs from the current editor state
const getEmbeddedUuids = (): string[] => {
    if (!editorView) return [];
    const uuids: string[] = [];
    editorView.state.doc.descendants((node) => {
        if (node.type.name === 'embedded_document' && node.attrs.documentUuid) {
            uuids.push(node.attrs.documentUuid);
        }
    });
    return [...new Set(uuids)];
};

// Sync embeddings to the backend
const syncEmbeddings = async () => {
    // Only sync for documentation pages (docId starts with "doc-")
    if (!props.docId.startsWith('doc-')) return;
    const pageId = parseInt(props.docId.replace('doc-', ''), 10);
    if (isNaN(pageId)) return;

    const embeddedUuids = getEmbeddedUuids();
    try {
        await apiClient.put(`/documentation/pages/${pageId}/embeddings`, {
            embedded_uuids: embeddedUuids,
        });
    } catch {
        // Silently fail - embeddings sync is best-effort
    }
};


// Refs for template
const editorElement = ref<HTMLElement | null>(null);
// Connection status is owned by the collab session store: it lives with
// the provider, which outlives this component's mount cycle, so the
// editor reads it rather than reconstructing it from socket events.
// `connectionError` is the one local override — a hard editor-construction
// failure surfaces as disconnected even when the socket itself is fine.
const collab = useCollabSessionStore();
const connectionError = ref(false);
const connectionStatus = computed<ConnectionStatus>(() =>
  connectionError.value
    ? 'disconnected'
    : collab.connectionStatus[props.docId] ?? 'connecting',
);

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
const mentionSelectedIndex = ref(0);

// Reactive query: empty while no `@` mention is active, otherwise the
// running query the ProseMirror plugin tracks. The composable watches
// this ref and runs (with debounce + AbortController cancellation)
// only when the value changes, so an idle editor costs nothing.
const mentionQuery = computed(() =>
    mentionState.value.active ? mentionState.value.query : '',
);
const { users: mentionUsers, isLoading: isMentionSearching } = useUserMentionSearch(
    mentionQuery,
    { limit: 8 },
);
watch(mentionUsers, () => {
    mentionSelectedIndex.value = 0;
});

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
let connectionErrorHandler: ((error: Event) => void) | null = null;
let connectionCloseHandler: ((event: WebSocketCloseEvent | null) => void) | null = null;
let syncedHandler: ((isSynced: boolean) => void) | null = null;
let statusReconnectHandler: ((event: { status: 'connected' | 'disconnected' | 'connecting' }) => void) | null = null;
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

const { position: typeMenuPosition, updatePosition: _updateTypeMenuPosition } =
    useDropdownPosition(typeButtonRef, showTypeMenu, { preferredWidth: 160 });
const { position: insertMenuPosition, updatePosition: _updateInsertMenuPosition } =
    useDropdownPosition(insertButtonRef, showInsertMenu, { preferredWidth: 180 });
const { position: _moreMenuPosition, updatePosition: _updateMoreMenuPosition } =
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
const mentionDropdownStyle = computed<Partial<Record<string, string>>>(() => {
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

// Mention handlers. Updating mentionState reactively drives the
// `mentionQuery` computed, which the composable watches; no explicit
// fetch call needed here.
const handleMentionStateChange = (state: MentionState) => {
    mentionState.value = state;
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
/**
 * Diagnostic update listener attached per editor mount. Tracked
 * here so we can `ydoc.off('update', ...)` on unmount, the listener
 * is cumulative on the shared doc (yjs ObservableV2 semantics), and
 * the underlying ydoc now outlives this component via the
 * useCollabSessionStore refcount.
 */
let updateDiagnosticHandler: ((update: Uint8Array, origin: unknown) => void) | null = null;
let updateV2DiagnosticHandler: ((update: Uint8Array) => void) | null = null;

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

    // Clear any prior construction error so a re-init starts neutral;
    // the connection status itself comes from the store.
    connectionError.value = false;

    try {
        log.info("Initializing collaborative editor with docId:", props.docId);

        // Single source of truth for the WS URL is `getCollabWsUrl`
        // so prewarm callers (RouterLink @mouseenter handlers) and
        // this editor agree on what to connect to.
        const baseWsUrl = getCollabWsUrl();

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

        // Acquire the shared Yjs session: the store either creates
        // a new (Y.Doc, WebsocketProvider, PermanentUserData) trio
        // for this docId or returns the existing one if a sibling
        // mount or this same component just released it within the
        // grace period. `isNew` gates the once-per-doc setup
        // (gc / setUserMapping). All other setup (event listeners,
        // awareness setLocalStateField, editor view) re-runs on
        // every mount, the previous editor instance's listeners
        // were torn off in `cleanup()`.
        const session = collab.acquire(props.docId, {
            baseWsUrl,
            providerParams: {
                resyncInterval: 20000,
                // Same-tab BC isn't needed in this SPA and removes
                // a class of duplicate-message edge cases.
                disableBc: true,
            },
        });
        ydoc = session.ydoc;
        provider = session.provider;
        permanentUserData = session.permanentUserData;

        // Diagnostic update listener (per-mount, removed in cleanup
        // so the cumulative ObservableV2 listener set doesn't leak
        // across mounts on the shared doc).
        updateDiagnosticHandler = (update: Uint8Array, origin: unknown) => {
            const originObj = origin as { constructor?: { name?: string } } | null;
            const isLocal = origin === null || origin === ydoc?.clientID ||
                (originObj?.constructor?.name === 'WebsocketProvider' ? false : true);
            log.info('🔄 YDOC UPDATE EVENT:', {
                updateSize: update.length,
                origin: originObj?.constructor?.name || String(origin) || 'null',
                isLikelyLocal: isLocal,
                yXmlFragmentLength: yXmlFragment?.length || 0,
                clientId: ydoc?.clientID,
                timestamp: new Date().toISOString(),
            });
            if (update.length < 100) {
                log.debug('   Update bytes:', Array.from(update).map(b => b.toString(16).padStart(2, '0')).join(' '));
            }
        };
        ydoc.on('update', updateDiagnosticHandler);

        // Set the user awareness field. `setLocalStateField` is the
        // merge variant; using `setLocalState({user:{...}})` would
        // overwrite y-prosemirror's `cursor` field on re-acquired
        // sessions (per y-protocols/awareness.js).
        provider.awareness.setLocalStateField('user', {
            name: getUserDisplayName(),
            color: getRandomColor(),
            uuid: authStore.user?.uuid || undefined,
            avatar: authStore.user?.avatar_thumb || authStore.user?.avatar_url || undefined,
        });

        // Map the Yjs client ID to the user UUID once per doc
        // lifetime. Y.PermanentUserData.setUserMapping appends to
        // a YArray with no dedup, calling it on every mount would
        // bloat the user map with duplicate entries.
        if (session.isNew && authStore.user?.uuid) {
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

        // 5. Mount the editor view immediately. y-prosemirror's
        //    ySyncPlugin observes the bound XmlFragment, so any
        //    updates that arrive AFTER mount (from IDB cache load
        //    or the websocket sync step) re-derive the PM doc on
        //    the fly. CRDT semantics make the order safe.
        //
        //    On a cold first paint this can briefly show empty
        //    content before the cache populates (~5-30ms warm,
        //    ~100ms cold per y-indexeddb timing). We accept the
        //    flash in exchange for deterministic mount ordering;
        //    the previous Promise.race gate added a window where
        //    the ws/ydoc lifecycle could observe a half-set-up
        //    editor.
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
                        cursorBuilder: (user: AwarenessUser | undefined, _clientId: number): HTMLElement => {
                            const cursor = document.createElement('span');
                            cursor.classList.add('ProseMirror-yjs-cursor');
                            cursor.setAttribute('style', `border-color: ${user?.color || '#808080'}`);
                            const userLabel = document.createElement('div');
                            userLabel.setAttribute('style', `background-color: ${user?.color || '#808080'}`);
                            userLabel.textContent = user?.name || 'Anonymous';
                            cursor.appendChild(userLabel);
                            return cursor;
                        },
                    }),
                    yUndoPlugin(),
                    createLinkTooltipPlugin({
                        onStateChange: (state) => {
                            linkTooltipState.value = state;
                        },
                    }),
                    createTicketLinkPlugin(),
                    createEmbeddedDocumentPlugin(),
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

        // Initial mention users pre-warm via the composable's
        // `immediate: true` watcher; no explicit prefetch needed.

        // 7. Connection status is owned by the collab session store: it
        // subscribes once per provider and derives the status from the
        // live socket state, so it stays correct across this component's
        // remounts (and the reused-provider case that used to latch
        // "disconnected"). `connectionStatus` here is just a computed
        // over it; nothing to wire, seed, or time out.

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
        connectionCloseHandler = (event: WebSocketCloseEvent | null) => {
            // y-websocket emits 'connection-close' with a null event
            // when disconnect() is called programmatically (e.g. tab
            // backgrounded, unmount). Only the network-driven close
            // path carries a real CloseEvent worth inspecting.
            if (!event) return;

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
        provider.on("sync", syncedHandler);

        // Note: Intentionally NOT overriding provider.ws.onmessage here.
        // y-websocket handles all sync messages (SYNC_STEP_1, SYNC_STEP_2, SYNC_UPDATE)
        // internally through its messageHandlers. Overriding onmessage can interfere
        // with the sync protocol and cause document content to not be applied correctly.

        // Track sync protocol errors which can cause disconnections
        // Monitor document updates to verify content is syncing.
        // Stored on a named handler reference so cleanup() can call
        // `ydoc.off('updateV2', ...)` — every editor mount adds one
        // of these to the shared ydoc, so without the detach a
        // long-lived session accumulates a listener per remount.
        updateV2DiagnosticHandler = (update: Uint8Array) => {
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
        };
        ydoc.on("updateV2", updateV2DiagnosticHandler);

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
        // Clean up on error — this runs `release(docId)` against
        // the store, balancing the `acquire` made above.
        cleanup();
        // The original retry path (`setTimeout(initEditor, 2000)`)
        // was a refcount leak: each retry called `acquire` again,
        // but the prior `cleanup` already released the refcount
        // from THIS mount's first acquire. The unbalanced acquire
        // accumulated one phantom reference per retry tick, and
        // the doc never reached refcount 0 even after unmount —
        // it lingered until LRU eviction.
        //
        // We rely on:
        //   * y-websocket's own reconnect for transient WS issues
        //     (capped at 5 attempts by statusReconnectHandler).
        //   * `idb.whenSynced` semantics for IDB load delays.
        //   * The connection-timeout watchdog in `onMounted` for
        //     hung handshakes.
        //
        // If construction errored for a non-transient reason
        // (bad docId, schema mismatch, plugin throw), retrying
        // 2s later wouldn't have helped anyway. Surface the
        // failure as a disconnected status and let the user
        // navigate away or refresh manually.
        connectionError.value = true;
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

// Build the active-viewers list from the awareness map.
//
// Two filtering rules, both required:
//   1. Drop entries with our own user UUID. y-websocket
//      reference server keeps awareness state for up to ~60s
//      after a socket dies (next ping cycle), so the prior
//      session of the same physical user lingers under a
//      DIFFERENT clientID and gets pushed to the new client
//      on connect. Filtering by `clientID` alone misses it
//      and the user sees themselves in the viewers list.
//      `outdatedTimeout` (30s) eventually GCs it but the gap
//      is user-visible.
//   2. Dedup remaining entries by user UUID. Same user
//      across multiple legitimate sessions (two open tabs)
//      should appear once, with the last-updated entry winning
//      (the awareness `lastUpdated` meta is the canonical
//      tiebreaker, but for simplicity we just take the most
//      recent occurrence in iteration order).
const updateConnectedUsers = () => {
    if (!provider) return;

    try {
        const states = provider.awareness.getStates();
        const currentUserUuid = authStore.user?.uuid;
        const seen = new Set<string>();
        if (currentUserUuid) seen.add(currentUserUuid);

        const users: { id: string; user: AwarenessUser }[] = [];
        states.forEach((state, clientId) => {
            const awarenessUser = state?.user as AwarenessUser | undefined;
            if (!awarenessUser?.name || typeof awarenessUser.name !== 'string') return;
            const uid = awarenessUser.uuid;
            if (uid && seen.has(uid)) return;
            if (uid) seen.add(uid);
            users.push({ id: String(clientId), user: awarenessUser });
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
            t('editor-code-block-language-prompt'),
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
    } catch {
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

    // CRITICAL: cleanup order matters now that the doc + provider
    // are owned by `useCollabSessionStore` and outlive the
    // component. We tear off only the listeners THIS mount
    // attached, then release the session refcount; the store
    // disconnects after a grace period and destroys after LRU
    // eviction. We never call `provider.destroy()`, `ydoc.destroy()`,
    // or `awareness.destroy()` here, those would torpedo the
    // shared session for siblings/future mounts.

    // 1. Destroy the editor view first so y-prosemirror plugins
    //    detach from the shared XmlFragment before we touch
    //    awareness or strip listeners.
    if (editorView) {
        try {
            editorView.destroy();
            editorView = null;
        } catch (e) {
            log.error("Error destroying editor view:", e);
        }
    }

    // 2. Detach this mount's provider listeners by reference.
    //    `Y.ObservableV2.off` is a strict ref-equality removal;
    //    skipping this would leak per-mount handlers across
    //    re-acquires of the same docId.
    if (provider) {
        try {
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
                provider.off("sync", syncedHandler);
                syncedHandler = null;
            }
            if (provider.awareness && awarenessChangeHandler) {
                provider.awareness.off("change", awarenessChangeHandler);
                awarenessChangeHandler = null;
            }
        } catch (e) {
            log.error("Error detaching provider listeners:", e);
        }
    }

    // 3. Detach the per-mount diagnostic update listeners from
    //    the shared ydoc. Two listeners are attached per mount
    //    (`update` and `updateV2`); without detaching both, every
    //    remount of the editor on the same docId stacks another
    //    handler on the shared doc, observably ballooning the
    //    listener list across long sessions.
    if (ydoc) {
        if (updateDiagnosticHandler) {
            try {
                ydoc.off('update', updateDiagnosticHandler);
            } catch (e) {
                log.error("Error detaching ydoc update listener:", e);
            }
            updateDiagnosticHandler = null;
        }
        if (updateV2DiagnosticHandler) {
            try {
                ydoc.off('updateV2', updateV2DiagnosticHandler);
            } catch (e) {
                log.error("Error detaching ydoc updateV2 listener:", e);
            }
            updateV2DiagnosticHandler = null;
        }
    }

    // 4. Release our refcount on the session. The store will
    //    disconnect the websocket after a grace period (so a
    //    quick nav-back reuses the same connection) and
    //    eventually destroy the doc + provider on LRU eviction.
    if (props.docId) {
        collab.release(props.docId);
    }

    // 5. Drop our local references; the store still holds them.
    provider = null;
    ydoc = null;
    permanentUserData = null;

    isInitialized.value = false;
    // connectionStatus is owned by the store and tied to the provider's
    // lifetime, so there's nothing to reset here — the computed simply
    // stops being read once this editor unmounts.
};

// Page-unload handling is owned by useCollabSessionStore (it
// iterates every active session and broadcasts `setLocalState(null)`
// for the awareness, then lets the OS close the WS). This
// component used to also disconnect the provider here, but the
// provider is shared across every component on the same docId —
// a sibling editor / hover-warmed prefetch / sidebar consumer would
// have its connection torpedoed by a single unmount. The store-
// owned cleanup is the right altitude; this stub is kept so the
// existing addEventListener / removeEventListener pairs in
// onMounted / onBeforeUnmount stay symmetrical without further
// churn.
const handleBeforeUnload = (_event: BeforeUnloadEvent) => {
    // intentionally empty — see comment above
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
    // Sync embeddings before unmounting (fire-and-forget)
    syncEmbeddings();

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

    // Mention search timer + AbortController teardown lives in the
    // composable's onScopeDispose; nothing extra to clean up here.

    // Remove visibility change listener
    document.removeEventListener("visibilitychange", handleVisibilityChange);
});

// Store original state when viewing revisions
let originalYXmlFragment: Y.XmlFragment | null = null;
let originalEditorState: EditorState | null = null;

// Revision viewing methods
function viewSnapshot(snapshotData: { revision_number: number; yjs_document_content: string }) {
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

        // Create a read-only state with the revision content. The
        // doc has no Yjs binding here (intentional — edits would be
        // discarded on exit), but ProseMirror would still accept
        // keystrokes into the local state if `editable` weren't
        // overridden, which presents a confusing "looks like I'm
        // typing, then it disappears" experience when the user
        // exits the revision view.
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

        // Update the editor view to show this read-only state and
        // mark it non-editable so dispatched transactions are
        // rejected at the prop boundary, matching the user's
        // expectation that historical revisions are immutable.
        editorView.updateState(readOnlyState);
        editorView.setProps({ editable: () => false });

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

        // Restore the original editor state (connected to live Yjs
        // doc) and flip the editable prop back on. The revision
        // view setter installed `editable: () => false`; without
        // this clear, the live doc would inherit the read-only
        // gate and silently refuse the next keystroke.
        editorView.updateState(originalEditorState);
        editorView.setProps({ editable: () => true });

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

// Handle revision selection from RevisionList.
//   null         -> user exited the revision view; restore live editor
//   revision_no  -> fetch that revision's Yjs snapshot, swap editor
//                   into read-only mode showing the historical doc
//
// Endpoint differs by surface: tickets vs documentation share the
// same response shape (ArticleRevisionDetail) but live under
// different paths. We pick based on docId prefix — same heuristic
// the rest of the editor uses.
const handleRevisionSelect = async (revisionNumber: number | null) => {
    if (revisionNumber === null) {
        log.info("Exiting revision view");
        try {
            exitRevisionView();
        } catch (error) {
            log.error("Failed to exit revision view:", error);
        }
        return;
    }

    log.info(`User selected revision ${revisionNumber}`);
    try {
        const isTicket = props.docId.startsWith('ticket-');
        const isDoc = props.docId.startsWith('doc-');
        let endpoint: string | null = null;
        if (isTicket) {
            const id = ticketId.value;
            if (id > 0) endpoint = `/collaboration/tickets/${id}/revisions/${revisionNumber}`;
        } else if (isDoc) {
            const id = parseInt(props.docId.replace('doc-', ''), 10);
            if (Number.isFinite(id) && id > 0) endpoint = `/collaboration/docs/${id}/revisions/${revisionNumber}`;
        }
        if (!endpoint) {
            log.error(`Unable to derive revision endpoint for docId=${props.docId}`);
            return;
        }
        const response = await apiClient.get<{
            revision_number: number;
            yjs_document_content: string;
        }>(endpoint);
        viewSnapshot(response.data);
    } catch (error) {
        log.error(`Failed to load revision ${revisionNumber}:`, error);
    }
};

// Handle revision restoration
const handleRevisionRestored = (revisionNumber: number) => {
    log.info(`Revision ${revisionNumber} restored successfully`);
    showRevisionHistory.value = false;
    // The backend broadcast will update all clients automatically
};

/**
 * Plain-text projection of the current ProseMirror document.
 * Used by the Insights panel to compute word/character/reading-
 * time stats without touching the editor's DOM. Returns the
 * document's `textContent` (block boundaries become single
 * spaces), which is good enough for stats — not a fidelity
 * export.
 */
function getTextContent(): string {
    if (!editorView) return ''
    return editorView.state.doc.textBetween(
        0,
        editorView.state.doc.content.size,
        '\n',
        ' ',
    )
}

// Expose methods and state for parent components
defineExpose({
    viewSnapshot,
    exitRevisionView,
    isViewingRevision,
    currentRevisionNumber,
    getTextContent,
    replaceAllWithEmbeddedDocument,
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
                    :title="$t('editor-toolbar-text-style')"
                    :aria-label="$t('editor-toolbar-text-style')"
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
                            {{ $t('editor-type-menu-plain') }}
                        </button>
                        <button
                            @click="
                                setHeading(1);
                                showTypeMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            {{ $t('editor-type-menu-heading-1') }}
                        </button>
                        <button
                            @click="
                                setHeading(2);
                                showTypeMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            {{ $t('editor-type-menu-heading-2') }}
                        </button>
                        <button
                            @click="
                                setHeading(3);
                                showTypeMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            {{ $t('editor-type-menu-heading-3') }}
                        </button>
                        <button
                            @click="
                                toggleBlockquote();
                                showTypeMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            {{ $t('editor-type-menu-blockquote') }}
                        </button>
                        <button
                            @click="
                                toggleCodeBlock();
                                showTypeMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            {{ $t('editor-type-menu-code-block') }}
                        </button>
                    </div>
                </Teleport>
            </div>

            <div class="toolbar-divider"></div>

            <!-- Formatting Buttons -->
            <button @click="toggleBold" class="toolbar-button" :title="$t('editor-toolbar-bold')" :aria-label="$t('editor-toolbar-bold')">
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
            <button @click="toggleItalic" class="toolbar-button" :title="$t('editor-toolbar-italic')" :aria-label="$t('editor-toolbar-italic')">
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
                :title="$t('editor-toolbar-bullet-list')"
                :aria-label="$t('editor-toolbar-bullet-list')"
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
                :title="$t('editor-toolbar-numbered-list')"
                :aria-label="$t('editor-toolbar-numbered-list')"
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
                    :title="$t('editor-toolbar-insert')"
                    :aria-label="$t('editor-toolbar-insert')"
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
                            {{ $t('editor-insert-menu-bullet-list') }}
                        </button>
                        <button
                            @click="
                                toggleOrderedList();
                                showInsertMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            {{ $t('editor-insert-menu-numbered-list') }}
                        </button>
                        <button
                            @click="
                                toggleBlockquote();
                                showInsertMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            {{ $t('editor-insert-menu-blockquote') }}
                        </button>
                        <button
                            @click="
                                toggleCodeBlock();
                                showInsertMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            {{ $t('editor-insert-menu-code-block') }}
                        </button>
                        <button
                            @click="
                                insertLink();
                                showInsertMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            {{ $t('editor-insert-menu-link') }}
                        </button>
                        <button
                            @click="
                                showDocumentPicker = true;
                                showInsertMenu = false;
                            "
                            class="dropdown-item"
                            role="menuitem"
                        >
                            {{ $t('editor-insert-menu-embed-document') }}
                        </button>
                    </div>
                </Teleport>
            </div>

            <div class="toolbar-divider"></div>

            <!-- Undo/Redo Buttons -->
            <button @click="undoEdit" class="toolbar-button" :title="$t('editor-toolbar-undo')" :aria-label="$t('editor-toolbar-undo')">
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
            <button @click="redoEdit" class="toolbar-button" :title="$t('editor-toolbar-redo')" :aria-label="$t('editor-toolbar-redo')">
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
                :title="$t('editor-toolbar-revision-history')"
                :aria-label="$t('editor-toolbar-revision-history')"
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
                <div class="text-xs text-tertiary mr-1">{{ $t('editor-toolbar-editing-with') }}</div>
                <div class="flex">
                    <div
                        v-for="(connectedUser, index) in connectedUsers"
                        :key="connectedUser.id"
                        class="flex items-center"
                        :style="{ marginLeft: index > 0 ? '-8px' : '0' }"
                        :title="
                            connectedUser.user.uuid
                                ? $t('editor-toolbar-user-title-uuid', { name: connectedUser.user.name, uuid: connectedUser.user.uuid })
                                : $t('editor-toolbar-user-title', { name: connectedUser.user.name })
                        "
                        @click="
                            () => {
                                console.log('User data:', connectedUser.user);
                            }
                        "
                    >
                        <UserAvatar
                            :uuid="connectedUser.user.uuid || null"
                            :fallbackName="connectedUser.user.name"
                            :fallbackAvatar="connectedUser.user.avatar"
                            :showName="false"
                            size="xs"
                            :clickable="!!connectedUser.user.uuid"
                        />
                    </div>
                </div>
            </div>

            <!-- Connection status indicator - v-show prevents layout shift on initial load -->
            <div v-show="connectionStatus === 'connecting'" class="connection-status-connecting">
                {{ $t('editor-toolbar-connection-connecting') }}
            </div>
            <div v-show="connectionStatus === 'disconnected'" class="connection-status-disconnected">
                {{ $t('editor-toolbar-connection-disconnected') }}
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
                        {{ $t('editor-mention-searching', { query: mentionState.query }) }}
                    </div>

                    <!-- Loading -->
                    <div v-if="isMentionSearching" class="px-3 py-4 flex items-center justify-center text-accent">
                        <Spinner />
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
                                :uuid="user.uuid"
                                :fallbackName="user.name"
                                :fallbackAvatar="user.avatar_thumb || user.avatar_url"
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
                        {{ $t('editor-mention-no-results') }}
                    </div>

                    <!-- Hint -->
                    <div class="px-3 py-2 text-xs text-tertiary border-t border-default bg-surface-alt flex items-center gap-4">
                        <span><kbd class="px-1 py-0.5 bg-surface rounded text-xs">↑↓</kbd> {{ $t('editor-mention-hint-navigate') }}</span>
                        <span><kbd class="px-1 py-0.5 bg-surface rounded text-xs">Enter</kbd> {{ $t('editor-mention-hint-select') }}</span>
                        <span><kbd class="px-1 py-0.5 bg-surface rounded text-xs">Esc</kbd> {{ $t('editor-mention-hint-close') }}</span>
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

        <!-- Document Picker for embedding -->
        <Teleport to="body">
            <DocumentPicker
                v-if="showDocumentPicker"
                @select="insertEmbeddedDocument"
                @close="showDocumentPicker = false"
            />
        </Teleport>
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
    /* Query container for the toolbar density tiers below. Safe: the
       editor is already a positioned containing block, and the toolbar
       dropdowns teleport to <body> so containment can't reach them. */
    container-type: inline-size;
    container-name: editor;
}

.toolbar {
    display: flex;
    padding: 0.5rem;
    background-color: var(--color-surface);
    border-bottom: 1px solid var(--color-default);
    gap: 0.25rem;
    align-items: center;
    overflow-x: auto;
}

.toolbar-button {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
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
    flex-shrink: 0;
    background-color: var(--color-default);
    margin: 0 0.5rem;
}

/* Density tiers: as the editor narrows, tighten the toolbar's gaps,
   button padding, and divider margins so the whole row fits a smaller
   container before the overflow-x scroll becomes the fallback. Driven
   by the editor's own width (container-name: editor), so the toolbar
   adapts wherever the editor is embedded, not by the viewport. */
@container editor (max-width: 620px) {
    .toolbar {
        padding: 0.375rem;
        gap: 0.125rem;
    }
    .toolbar-button {
        padding: 0.25rem 0.375rem;
    }
    .toolbar-divider {
        height: 1.25rem;
        margin: 0 0.25rem;
    }
}

@container editor (max-width: 460px) {
    .toolbar {
        padding: 0.25rem;
        gap: 0.0625rem;
    }
    .toolbar-button {
        padding: 0.25rem 0.3125rem;
    }
    .toolbar-divider {
        margin: 0 0.125rem;
    }
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
    z-index: 300; /* z-overlay */
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

.connection-status-connecting,
.connection-status-disconnected {
    font-size: 0.75rem;
    font-weight: 500;
    flex-shrink: 0;
    white-space: nowrap;
    padding: 0.25rem 0.625rem;
    border-radius: 0.375rem;
}

.connection-status-connecting {
    color: var(--color-status-warning, #f59e0b);
    background-color: var(--color-status-warning-bg, rgba(245, 158, 11, 0.15));
    border: 1px solid var(--color-status-warning-border, rgba(245, 158, 11, 0.3));
}

.connection-status-disconnected {
    color: var(--color-status-error);
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
    z-index: 300; /* z-overlay */
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
    font-family: var(--font-sans, 'Inter', ui-sans-serif, system-ui, sans-serif);
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
    font-family: var(--font-sans, 'Inter', ui-sans-serif, system-ui, sans-serif);
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
/* Compact inline ticket card */
.ProseMirror .ticket-link-card {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 10px;
    margin: 2px 0;
    background: var(--color-surface-alt);
    border: 1px solid var(--color-border-subtle);
    border-radius: 4px;
    font-size: 12px;
    line-height: 1.3;
    cursor: pointer;
    transition: border-color 0.15s ease, background 0.15s ease;
    max-width: 100%;
    vertical-align: middle;
}

.ProseMirror .ticket-link-card:hover {
    border-color: var(--color-accent);
    background: var(--color-surface-hover);
}

.ProseMirror .ticket-link-loading {
    opacity: 0.6;
}

.ProseMirror .ticket-link-error {
    border-color: var(--color-status-error);
}

.ProseMirror .ticket-link-id {
    font-weight: 600;
    color: var(--color-text-tertiary, #888);
    font-family: var(--font-mono, monospace);
    font-size: 11px;
    flex-shrink: 0;
}

.ProseMirror .ticket-link-title {
    color: var(--color-text-primary, #fff);
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
}

/* Status & priority badges */
.ProseMirror .ticket-link-status,
.ProseMirror .ticket-link-priority {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 3px;
    font-weight: 500;
    text-transform: capitalize;
    flex-shrink: 0;
    white-space: nowrap;
}

.ProseMirror .ticket-link-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    background: currentColor;
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

/* Loading spinner */
.ProseMirror .ticket-link-loader {
    width: 10px;
    height: 10px;
    border: 1.5px solid var(--color-border-default, #333);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: ticket-link-spin 0.8s linear infinite;
}

@keyframes ticket-link-spin {
    to { transform: rotate(360deg); }
}

/* Embedded Document */
.ProseMirror .embedded-document-block {
    margin: 0.5em 0;
    border-left: 3px solid var(--color-accent, #3b82f6);
}

.ProseMirror .embedded-document-block:hover {
    border-left-color: var(--color-accent-hover, #2563eb);
}

.embedded-doc-header {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 8px;
    font-size: 10px;
    color: var(--color-text-tertiary, #888);
    cursor: pointer;
    user-select: none;
}

.embedded-doc-header:hover .embedded-doc-title {
    color: var(--color-accent);
}

.embedded-doc-header:hover .embedded-doc-open {
    opacity: 1;
}

.embedded-doc-title {
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
}

.embedded-doc-open {
    display: flex;
    align-items: center;
    opacity: 0;
    transition: opacity 0.15s ease;
    cursor: pointer;
}

.embedded-doc-open:hover {
    color: var(--color-accent);
}

.embedded-doc-loader {
    width: 10px;
    height: 10px;
    border: 1.5px solid var(--color-border-default, #333);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: ticket-link-spin 0.8s linear infinite;
}

.embedded-doc-content {
    padding: 0 8px;
}

.embedded-doc-content > :first-child {
    margin-top: 0;
}

.embedded-doc-content > :last-child {
    margin-bottom: 0;
}

/* Skeleton loading */
.embedded-doc-skeleton {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 2px 0;
}

.skeleton-line {
    height: 10px;
    background: var(--color-surface-hover);
    border-radius: 3px;
    animation: skeleton-pulse 1.5s ease-in-out infinite;
}

@keyframes skeleton-pulse {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 0.8; }
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
    font-family: var(--font-sans, 'Inter', ui-sans-serif, system-ui, sans-serif);
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
  }

  .ProseMirror .ticket-link-id,
  .ProseMirror .ticket-link-title {
    color: #000 !important;
  }

  .ProseMirror .ticket-link-status,
  .ProseMirror .ticket-link-priority {
    background: transparent !important;
    border: 1px solid currentColor !important;
  }

  .ProseMirror .ticket-link-dot {
    display: none !important;
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
