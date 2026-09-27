/** A request type a workspace offers requesters (an admin-flagged category). */
export interface RequestTypeOption {
  id: number
  name: string
  description: string | null
  color: string | null
}
