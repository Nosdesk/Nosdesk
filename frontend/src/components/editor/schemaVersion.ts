/**
 * The document schema's version. The collaboration server holds a table of
 * the first version that knows each node type, and refuses a client below a
 * document's requirement, so an older client never receives (and deletes) a
 * node it can't build. 2 = tables.
 *
 * Its own module so the collab session store can declare it on the socket
 * without pulling the ProseMirror schema into every chunk that warms a note.
 */
export const EDITOR_SCHEMA_VERSION = 2;

/** The version that brought tables. A later server still allows them. */
export const TABLES_SCHEMA_VERSION = 2;
