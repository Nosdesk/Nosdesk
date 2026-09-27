/** A request type a workspace offers requesters (an admin-flagged category). */
export interface RequestTypeOption {
  id: number
  name: string
  description: string | null
  color: string | null
}

/** A help article suggested while a requester writes a new request. */
export interface ArticleHit {
  id: number
  title: string
  slug: string
}
