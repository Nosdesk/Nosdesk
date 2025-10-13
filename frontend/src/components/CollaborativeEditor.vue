<script setup lang="ts">
// Collaborative Editor with Yjs for real-time document editing
//
// Logging behavior:
// - Minimal logging by default (info, warn, error only)
// - Debug logging enabled in development mode or when localStorage['editor-verbose-logging'] = 'true'
// - To enable verbose logging in production: localStorage.setItem('editor-verbose-logging', 'true')
// - To disable: localStorage.removeItem('editor-verbose-logging')

import { ref, onMounted, onBeforeUnmount, watch, computed } from "vue";
import * as Y from "yjs";
import { PermanentUserData } from "yjs";
import { WebsocketProvider } from "y-websocket";
import { EditorView } from "prosemirror-view";
import { EditorState, Selection } from "prosemirror-state";
import { schema } from "@/components/editor/schema";
import { useAuthStore } from "@/stores/auth";
import UserAvatar from "./UserAvatar.vue";
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
    chainCommands,
    exitCode,
    createParagraphNear,
    liftEmptyBlock,
    splitBlock,
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
import { baseKeymap } from "prosemirror-commands";
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

// Props
interface Props {
    docId: string;
}

const props = defineProps<Props>();

// Get auth store for user info
const authStore = useAuthStore();

// Computed property for connection status
const connectionStatus = computed(() => {
    return isConnected.value ? "Connected" : "Disconnected";
});

// Refs for template
const editorElement = ref<HTMLElement | null>(null);
const isConnected = ref(false);

// State for connected users
const connectedUsers = ref<{ id: string; user: any }[]>([]);

// Remove save status tracking since backend handles saves automatically

// Track initialization state
const isInitialized = ref(false);
let reinitializeTimeout: ReturnType<typeof setTimeout> | null = null;


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

// Global variables - mirroring the demo approach exactly
let ydoc: Y.Doc | null = null;
let provider: WebsocketProvider | null = null;
let yXmlFragment: Y.XmlFragment | null = null;
let editorView: EditorView | null = null;

// Enhanced logging
const log = {
    info: (message: string, ...args: any[]) =>
        console.log(`[YJS-Editor] ${message}`, ...args),
    error: (message: string, ...args: any[]) =>
        console.error(`[YJS-Editor] ${message}`, ...args),
    debug: (message: string, ...args: any[]) => {
        // Only log debug messages in development or when verbose logging is enabled
        if (
            import.meta.env.DEV ||
            window.localStorage.getItem("editor-verbose-logging") === "true"
        ) {
            console.debug(`[YJS-Editor] ${message}`, ...args);
        }
    },
    warn: (message: string, ...args: any[]) =>
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
    const keys: { [key: string]: any } = {};

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

        // 2. Create the websocket provider
        // Note: y-websocket automatically appends `/${docId}` to the URL we provide
        // So we need to provide the base URL without the document ID
        const baseWsUrl =
            import.meta.env.VITE_WS_SERVER_URL ||
            `${window.location.protocol === "https:" ? "wss:" : "ws:"}//${
                window.location.hostname
            }:8080/api/collaboration/ws`;

        // Get JWT token for authentication
        const token = localStorage.getItem("token");
        if (!token) {
            log.error("No authentication token found. Please log in.");
            return;
        }

        log.debug("WebSocket connection details:", {
            baseUrl: baseWsUrl,
            documentId: props.docId,
            hasToken: !!token,
        });

        // Create WebsocketProvider with custom URL construction
        // y-websocket will append /${docId} to the baseWsUrl, creating the correct backend route
        provider = new WebsocketProvider(baseWsUrl, props.docId, ydoc, {
            params: { token: token },
            // Set resync interval to 20 seconds to prevent 30-second timeout disconnects
            // y-websocket closes connection if no message received in 30s, so we need
            // periodic Yjs protocol messages to keep the connection alive
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
            },
        });

        // 4. Get the XML fragment and initialize ProseMirror document
        yXmlFragment = ydoc.getXmlFragment("prosemirror");

        // Initialize ProseMirror with the Yjs binding
        const { doc, mapping } = initProseMirrorDoc(yXmlFragment, schema);

        // Verify the document was initialized correctly
        if (!doc) {
            throw new Error(
                "Failed to initialize ProseMirror document from Yjs",
            );
        }

        // 5. Create the editor view - it's safe to create even before sync
        // The ySyncPlugin will handle updates when the document syncs
        if (!editorElement.value) {
            throw new Error("Editor element became null during initialization");
        }

        editorView = new EditorView(editorElement.value, {
            state: EditorState.create({
                doc: doc,
                schema,
                plugins: [
                    ySyncPlugin(yXmlFragment, { mapping }),
                    yCursorPlugin(provider.awareness),
                    yUndoPlugin(),
                    keymap({
                        "Mod-z": undo,
                        "Mod-y": redo,
                        "Mod-Shift-z": redo,
                        "Mod-b": toggleMark(schema.marks.strong),
                        "Mod-i": toggleMark(schema.marks.em),
                        "Mod-Alt-c": setBlockType(schema.nodes.code_block),
                        // Exit code block with triple backticks
                        "```": (state, dispatch) => {
                            const { $from } = state.selection;
                            if (
                                $from.parent.type === schema.nodes.code_block &&
                                dispatch
                            ) {
                                // Check if we're at the end of a code block
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
                        // Better Enter handling in code blocks
                        Enter: (state, dispatch) => {
                            const { $from } = state.selection;
                            if ($from.parent.type === schema.nodes.code_block) {
                                if (dispatch) {
                                    dispatch(state.tr.insertText("\n"));
                                }
                                return true;
                            }
                            return false;
                        },
                    }),
                    // Add list handling keymap - this is crucial for proper list behavior
                    keymap(createListKeymap(schema)),
                    // Add individual plugins instead of exampleSetup
                    buildInputRules(schema), // Custom markdown input rules
                    keymap(baseKeymap), // Basic key bindings
                    dropCursor(), // Shows cursor when dragging
                    // NOTE: gapCursor() removed - causes null reference errors with empty Yjs documents
                ],
            }),
        });

        // 7. Set up connection status handler with enhanced logging
        provider.on(
            "status",
            (event: {
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

                // Log additional context for disconnections
                if (event.status === "disconnected") {
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
                } else if (event.status === "connected") {
                    log.info("WebSocket connected successfully");
                }
            },
        );

        // Add error event handler for more detailed error information
        provider.on("connection-error", (error: any) => {
            log.error("WebSocket connection error:", error);
            log.debug("Error details:", {
                message: error?.message || "No error message",
                code: error?.code || "No error code",
                type: error?.type || "No error type",
                target: error?.target || "No target info",
                timestamp: new Date().toISOString(),
            });
        });

        // Monitor for authentication-related disconnections
        provider.on("connection-close", (event: any) => {
            log.warn("WebSocket connection closed:", {
                code: event?.code,
                reason: event?.reason,
                wasClean: event?.wasClean,
                timestamp: new Date().toISOString(),
            });

            // Check for authentication-related close codes
            if (event?.code === 1008) {
                log.error(
                    "WebSocket closed due to policy violation - likely authentication failure",
                );
                log.error(
                    "Check if JWT token is valid and user still exists in database",
                );
            } else if (event?.code === 1011) {
                log.error(
                    "WebSocket closed due to server error - likely backend database/processing issue",
                );
            } else if (event?.code === 1006) {
                log.warn(
                    "WebSocket closed abnormally - network issue or server crash",
                );
            }
        });

        // Track sync protocol errors which can cause disconnections
        // Remove the frequent document update logging
        // ydoc.on("updateV2", (update: Uint8Array) => {
        //   log.debug("Document update:", {
        //     updateSize: update.length,
        //     timestamp: new Date().toISOString(),
        //   });
        // });

        // Add retry logic monitoring - simplified
        let reconnectAttempts = 0;
        const maxReconnectAttempts = 5;
        let reconnectTimeout: ReturnType<typeof setTimeout> | null = null;

        provider.on(
            "status",
            (event: {
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
            },
        );

        // 7. Add awareness change listener to update connected users
        provider.awareness.on("change", () => {
            updateConnectedUsers();
        });

        // 8. Note: Save status tracking removed since backend handles saves automatically via Redis

        // 9. For debugging purposes
        window.example = {
            provider,
            ydoc,
            yXmlFragment,
            editorView,
            diagnoseConnection: diagnoseConnectionIssue,
        };

        // Add direct WebSocket event monitoring only in debug mode
        if (
            (import.meta.env.DEV ||
                window.localStorage.getItem("editor-verbose-logging") ===
                    "true") &&
            provider &&
            provider.ws
        ) {
            const originalOnClose = provider.ws.onclose;
            provider.ws.onclose = (event: CloseEvent) => {
                log.debug("WebSocket close event:", {
                    code: event.code,
                    reason: event.reason || "No reason provided",
                    wasClean: event.wasClean,
                    closeCodeMeaning: getCloseCodeMeaning(event.code),
                });

                // Call original handler if it exists
                if (originalOnClose && provider?.ws) {
                    originalOnClose.call(provider.ws, event);
                }
            };

            const originalOnError = provider.ws.onerror;
            provider.ws.onerror = (event: Event) => {
                log.debug("WebSocket error event:", {
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
        const users: { id: string; user: any }[] = [];

        // Convert Map to array and exclude the current user
        states.forEach((state, clientId) => {
            if (
                state &&
                state.user &&
                provider &&
                clientId !== provider.awareness.clientID
            ) {
                // Only include users with valid user data
                if (state.user.name && typeof state.user.name === "string") {
                    users.push({
                        id: String(clientId),
                        user: state.user,
                    });
                }
            }
        });

        connectedUsers.value = users;
    } catch (error) {
        log.error("Error updating connected users:", error);
    }
};

// Simple function to focus the editor
const focusEditor = () => {
    if (editorView) {
        editorView.focus();
        console.log('Editor focused, doc content:', editorView.state.doc.textContent);
        console.log('Editor DOM:', editorView.dom.innerHTML);
        console.log('Has focus?', document.activeElement === editorView.dom);
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

// Handle tab visibility changes to prevent wasteful reconnect cycles
// When browser backgrounds tab, timers get throttled causing irregular resyncs
// This leads to timeout->reconnect->timeout cycles. Clean disconnect is better.
const handleVisibilityChange = () => {
    if (document.hidden && provider?.wsconnected) {
        log.info("Tab backgrounded - disconnecting WebSocket to save resources");
        provider.disconnect();
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

    // Check if we're already in a code block
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

const insertLink = () => {
    if (!editorView) return;
    const { state, dispatch } = editorView;
    const url = prompt("Enter URL for the link:", "https://");
    if (url) {
        const { from, to } = state.selection;
        const tr = state.tr;
        if (from === to) {
            const text = prompt("Enter link text:", "Link");
            if (text) {
                tr.insertText(text, from, from);
                tr.addMark(
                    from,
                    from + text.length,
                    schema.marks.link.create({ href: url }),
                );
            }
        } else {
            tr.addMark(from, to, schema.marks.link.create({ href: url }));
        }
        dispatch(tr);
    }
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
            provider.off("status", null as any);
            provider.off("connection-error", null as any);
            provider.off("connection-close", null as any);

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
                            // let's try to inspect them directly
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

    // Environment configuration
    const baseWsUrl =
        import.meta.env.VITE_WS_SERVER_URL ||
        `${window.location.protocol === "https:" ? "wss:" : "ws:"}//${window.location.hostname}:8080/api/collaboration/ws`;

    log.info("Environment Configuration:", {
        nodeEnv: import.meta.env.NODE_ENV,
        mode: import.meta.env.MODE,
        wsServerUrl:
            import.meta.env.VITE_WS_SERVER_URL ||
            "Not set (using auto-detected)",
        computedWsUrl: baseWsUrl,
        windowLocation: {
            hostname: window.location.hostname,
            port: window.location.port,
            protocol: window.location.protocol,
            href: window.location.href,
        },
    });

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
    if (!token) {
        log.error("❌ No authentication token found - Please log in again");
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

// Add window debug methods
window.example = undefined; // Initialize with undefined until editor is created

// Update the global interface
declare global {
    interface Window {
        example?: any;
    }
}

onMounted(() => {
    initEditor();
    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("keydown", handleKeydown);
    window.addEventListener("beforeunload", handleBeforeUnload);

    // Add network status monitoring
    window.addEventListener("online", () => {
        log.info(
            "Network came back online - websocket may reconnect automatically",
        );
    });

    window.addEventListener("offline", () => {
        log.warn("Network went offline - websocket connection will be lost");
    });

    // Add visibility change listener (handler defined at top level)
    document.addEventListener("visibilitychange", handleVisibilityChange);
});

onBeforeUnmount(() => {
    cleanup();
    document.removeEventListener("mousedown", handleClickOutside);
    document.removeEventListener("keydown", handleKeydown);
    window.removeEventListener("beforeunload", handleBeforeUnload);

    // Remove network status monitoring
    window.removeEventListener("online", () => {});
    window.removeEventListener("offline", () => {});

    // Remove visibility change listener
    document.removeEventListener("visibilitychange", handleVisibilityChange);
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
                <div
                    v-if="showTypeMenu"
                    ref="typeMenuRef"
                    class="dropdown-menu"
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
                <div
                    v-if="showInsertMenu"
                    ref="insertMenuRef"
                    class="dropdown-menu"
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

            <!-- Spacer to push connection controls to right -->
            <div class="flex-grow"></div>

            <!-- Connected users -->
            <div
                v-if="connectedUsers.length > 0"
                class="flex items-center gap-1 mr-2"
            >
                <div class="text-xs text-slate-300 mr-1">Editing with:</div>
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
                            :showName="false"
                            size="xs"
                            :clickable="!!connectedUser.user.uuid"
                        />
                    </div>
                </div>
            </div>

            <!-- Connection status indicator only -->
            <div class="connection-status" :class="{ connected: isConnected }">
                {{ connectionStatus }}
            </div>
        </div>

        <!-- Editor content with click handler -->
        <div
            id="editor"
            ref="editorElement"
            @click="focusEditor"
            class="editor-container"
        ></div>
    </div>
</template>

<style>
.collaborative-editor {
    display: flex;
    flex-direction: column;
    border-radius: 0 0 0.75rem 0.75rem;
    overflow: hidden;
    background-color: #1c283d; /* bg-slate-800 */
    height: 100%;
    width: 100%;
    position: relative;
}

.toolbar {
    display: flex;
    padding: 0.5rem;
    background-color: rgb(51 65 85 / 0.3); /* bg-slate-700/30 */
    border-bottom: 1px solid rgb(51 65 85 / 0.5); /* border-slate-700/50 */
    flex-wrap: wrap;
    gap: 0.25rem;
    align-items: center;
}

.toolbar-button {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0.25rem 0.5rem;
    background-color: rgb(51 65 85 / 0.5); /* bg-slate-700/50 */
    border: none;
    border-radius: 0.375rem; /* rounded-md */
    color: rgb(148 163 184); /* text-slate-400 */
    cursor: pointer;
    font-size: 0.875rem;
    transition: all 0.2s;
}

.toolbar-button:hover {
    background-color: rgb(51 65 85); /* bg-slate-700 */
    color: rgb(248 250 252); /* text-white */
}

.toolbar-button.active {
    color: rgb(59 130 246); /* text-blue-500 */
}

.toolbar-divider {
    width: 1px;
    height: 1.5rem;
    background-color: rgb(148 163 184); /* bg-slate-400 */
    margin: 0 0.5rem;
}

.dropdown-menu {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 0.25rem;
    width: 12rem;
    background-color: rgb(30 41 59); /* bg-slate-800 */
    border: 1px solid rgb(51 65 85 / 0.5); /* border-slate-700/50 */
    border-radius: 0.5rem; /* rounded-lg */
    box-shadow:
        0 10px 15px -3px rgba(0, 0, 0, 0.1),
        0 4px 6px -2px rgba(0, 0, 0, 0.05);
    z-index: 50;
    overflow: hidden;
}

.dropdown-item {
    display: block;
    width: 100%;
    padding: 0.5rem 1rem;
    text-align: left;
    font-size: 0.875rem;
    color: rgb(226 232 240); /* text-slate-200 */
    background-color: transparent;
    border: none;
    cursor: pointer;
    transition: background-color 0.2s;
}

.dropdown-item:hover {
    background-color: rgb(51 65 85 / 0.5); /* bg-slate-700/50 */
    color: rgb(248 250 252); /* text-white */
}

.connection-status {
    font-size: 0.875rem;
    color: rgb(239 68 68); /* text-red-500 */
    padding: 0.25rem 0.5rem;
    border-radius: 0.25rem;
    background-color: rgb(127 29 29 / 0.2); /* bg-red-900/20 */
}

.connection-status.connected {
    color: rgb(34 197 94); /* text-green-500 */
    background-color: rgb(20 83 45 / 0.2); /* bg-green-900/20 */
}

.editor-container {
    position: relative;
    background-color: #1c283d; /* bg-slate-800 */
    border-radius: 0.5rem;
    color: rgb(248 250 252); /* text-slate-50 */
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
}

/* Force cursor visibility in Chrome for empty contenteditable */
.ProseMirror:empty:before {
    content: '';
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
    border: 1px solid rgb(79 70 229); /* border-indigo-600 */
    border-radius: 0.5rem;
}

/* Ensure toolbar doesn't restrict editor content */
.editor-toolbar {
    position: sticky;
    top: 0;
    z-index: 10;
    background-color: rgb(30 41 59); /* bg-slate-800 */
    border-top-left-radius: 0.5rem;
    border-top-right-radius: 0.5rem;
    border-bottom: 1px solid rgb(51 65 85); /* border-slate-700 */
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
    border-bottom: 1px solid rgb(51 65 85); /* border-slate-700 */
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
    border-left: 4px solid rgb(59 130 246); /* border-blue-500 */
    padding-left: 1rem;
    margin-left: 0;
    margin-right: 0;
    color: rgb(148 163 184); /* text-slate-400 */
    margin-top: 1rem;
    margin-bottom: 1rem;
}

.ProseMirror pre {
    background-color: rgb(15 23 42); /* bg-slate-900 */
    padding: 0.75rem;
    border-radius: 0.5rem; /* rounded-lg */
    overflow-x: auto;
    font-family:
        ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas,
        "Liberation Mono", "Courier New", monospace;
    margin-top: 1rem;
    margin-bottom: 1rem;
    border: 1px solid rgb(51 65 85 / 0.3); /* border-slate-700/30 */
    position: relative;
}

/* Language indicator for code blocks */
.ProseMirror pre[data-language]::before {
    content: attr(data-language);
    position: absolute;
    top: 0;
    right: 0;
    padding: 0.25rem 0.5rem;
    background-color: rgb(51 65 85 / 0.5);
    color: rgb(148 163 184);
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
    color: rgb(226 232 240); /* text-slate-200 */
    display: block;
    overflow-x: auto;
    white-space: pre;
}

/* Better syntax highlighting colors for common languages */
.ProseMirror pre code.language-javascript,
.ProseMirror pre code.language-js,
.ProseMirror pre code.language-typescript,
.ProseMirror pre code.language-ts {
    color: rgb(125 211 252); /* text-sky-300 */
}

.ProseMirror pre code.language-python,
.ProseMirror pre code.language-py {
    color: rgb(134 239 172); /* text-green-300 */
}

.ProseMirror pre code.language-html,
.ProseMirror pre code.language-xml {
    color: rgb(251 146 60); /* text-orange-400 */
}

.ProseMirror pre code.language-css,
.ProseMirror pre code.language-scss {
    color: rgb(147 197 253); /* text-blue-300 */
}

.ProseMirror pre code.language-bash,
.ProseMirror pre code.language-sh,
.ProseMirror pre code.language-shell {
    color: rgb(163 230 53); /* text-lime-400 */
}

.ProseMirror pre code.language-json {
    color: rgb(252 211 77); /* text-amber-300 */
}

.ProseMirror pre code.language-sql {
    color: rgb(196 181 253); /* text-violet-300 */
}

.ProseMirror code {
    background-color: rgb(51 65 85 / 0.5); /* bg-slate-700/50 */
    padding: 0.125rem 0.375rem;
    border-radius: 0.25rem;
    font-family:
        ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas,
        "Liberation Mono", "Courier New", monospace;
    color: rgb(226 232 240); /* text-slate-200 */
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
    color: rgb(226 232 240); /* text-slate-200 */
}

.ProseMirror ul ul {
    list-style-type: circle;
}

.ProseMirror ul ul ul {
    list-style-type: square;
}

.ProseMirror ol {
    list-style-type: decimal;
    color: rgb(226 232 240); /* text-slate-200 */
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
    color: rgb(59 130 246); /* text-blue-500 */
    text-decoration: underline;
}

.ProseMirror a:hover {
    color: rgb(96 165 250); /* text-blue-400 */
}

.ProseMirror strong {
    font-weight: 700;
    color: rgb(226 232 240); /* text-slate-200 */
}

.ProseMirror em {
    font-style: italic;
    color: rgb(226 232 240); /* text-slate-200 */
}

.ProseMirror .yRemoteSelection {
    position: absolute;
    border-left: 2px solid;
    border-right: 2px solid;
    pointer-events: none;
    opacity: 0.5;
    background-color: rgba(
        59,
        130,
        246,
        0.2
    ); /* Add a subtle background for selection */
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

</style>
