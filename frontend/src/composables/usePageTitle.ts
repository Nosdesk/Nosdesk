import {
  computed,
  getCurrentInstance,
  getCurrentScope,
  onActivated,
  onDeactivated,
  onScopeDispose,
  shallowRef,
  toValue,
  type MaybeRefOrGetter,
} from 'vue';

export interface TitleableDocument {
  id: string;
  title: string;
  icon: string;
  slug?: string;
}

export interface TitleableTicket {
  id: number;
  number?: number;
  title: string;
  [key: string]: any; // Allow other properties from the reactive ticket object
}

export interface TitleableDevice {
  id: number;
  name: string;
  attributes?: Record<string, unknown>;
  [key: string]: any; // Allow other properties from the reactive device object
}

export type TitleSaveHandler = (value: string) => Promise<void>;

/**
 * What a view puts in the site header and the browser tab. A claim names
 * one subject: a ticket, an asset, a document, or a plain title (a list
 * page, a project, a PDF).
 */
export interface PageTitleClaim {
  title?: MaybeRefOrGetter<string | null | undefined>;
  ticket?: MaybeRefOrGetter<TitleableTicket | null | undefined>;
  device?: MaybeRefOrGetter<TitleableDevice | null | undefined>;
  document?: MaybeRefOrGetter<TitleableDocument | null | undefined>;
  /** Persists a rename typed into the site header. */
  onTitleSave?: TitleSaveHandler;
  /** Whether the header offers the rename. Defaults to true when onTitleSave is set. */
  titleEditable?: MaybeRefOrGetter<boolean>;
  /** Persists a document icon picked in the site header. */
  onIconSave?: TitleSaveHandler;
}

export interface ClaimRecord {
  id: number;
  claim: PageTitleClaim;
}

let nextId = 0;
// Every mounted claim, oldest first, and the ones a KeepAlive has put away.
const claims = shallowRef<ClaimRecord[]>([]);
const parked = shallowRef<ReadonlySet<number>>(new Set());

function hasSubject(claim: PageTitleClaim): boolean {
  return Boolean(
    toValue(claim.title) || toValue(claim.ticket) || toValue(claim.device) || toValue(claim.document),
  );
}

/**
 * The claim that owns the header: the newest live one with something to
 * show. A view that is still loading falls through to the claim beneath it
 * (its parent, or the view it is replacing while that one unmounts) rather
 * than flashing the route's generic title.
 */
export const activeClaim = computed<ClaimRecord | null>(() => {
  const list = claims.value;
  for (let i = list.length - 1; i >= 0; i--) {
    const record = list[i];
    if (!parked.value.has(record.id) && hasSubject(record.claim)) return record;
  }
  return null;
});

/**
 * Claim the page title for as long as the calling component (or effect
 * scope) lives. Nothing needs clearing: the claim goes when its owner
 * unmounts, and steps aside while a KeepAlive holds its owner deactivated.
 */
export function usePageTitle(claim: PageTitleClaim): void {
  const record: ClaimRecord = { id: nextId++, claim };
  claims.value = [...claims.value, record];

  const setParked = (isParked: boolean) => {
    const next = new Set(parked.value);
    if (isParked) next.add(record.id);
    else next.delete(record.id);
    parked.value = next;
  };

  if (getCurrentInstance()) {
    onDeactivated(() => setParked(true));
    onActivated(() => setParked(false));
  }
  if (getCurrentScope()) {
    onScopeDispose(() => {
      claims.value = claims.value.filter((r) => r.id !== record.id);
      setParked(false);
    });
  }
}
