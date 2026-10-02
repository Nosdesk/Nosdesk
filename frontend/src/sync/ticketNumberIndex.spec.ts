/**
 * The pool resolves a ticket number to its id, following the ticket rows as
 * they arrive, change and go.
 */
import { beforeEach, describe, expect, it } from 'vitest'
import * as pool from '@nosdesk/core/sync/pool'

describe('ticketIdForNumber', () => {
  beforeEach(() => pool.reset())

  it('finds a pooled ticket by its number', () => {
    pool.upsert('ticket', 40, { id: 40, number: 7, title: 'Printer jammed' })
    expect(pool.ticketIdForNumber(7)).toBe(40)
    expect(pool.ticketIdForNumber(40)).toBeUndefined()
  })

  it('follows a renumbered ticket', () => {
    pool.upsert('ticket', 40, { id: 40, number: 7 })
    pool.patch('ticket', 40, { number: 8 })
    expect(pool.ticketIdForNumber(7)).toBeUndefined()
    expect(pool.ticketIdForNumber(8)).toBe(40)
  })

  it('forgets a removed ticket, and everything on reset', () => {
    pool.upsert('ticket', 40, { id: 40, number: 7 })
    pool.upsert('ticket', 41, { id: 41, number: 8 })
    pool.remove('ticket', 40)
    expect(pool.ticketIdForNumber(7)).toBeUndefined()
    expect(pool.ticketIdForNumber(8)).toBe(41)
    pool.reset()
    expect(pool.ticketIdForNumber(8)).toBeUndefined()
  })

  it('ignores other aggregates', () => {
    pool.upsert('project', 3, { id: 3, number: 7 })
    expect(pool.ticketIdForNumber(7)).toBeUndefined()
  })
})
