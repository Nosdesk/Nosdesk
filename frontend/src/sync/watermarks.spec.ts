/**
 * Watermark writes read the whole map and write it back, so they run one
 * after another: a set that interleaves with a drop never brings the dropped
 * group back.
 */
import { describe, expect, it, vi } from 'vitest'
import type { IdbHandle } from './idb'

/** A meta store whose reads and writes each take a turn of the event loop,
 * so two unserialized read-modify-writes interleave. */
const fake = vi.hoisted(() => {
  const tick = () => new Promise((resolve) => setTimeout(resolve, 0))
  const store = { marks: {} as Record<string, number> }
  return {
    store,
    idb: {
      getGroupWatermarks: async () => {
        const read = { ...store.marks }
        await tick()
        return read
      },
      replaceGroupWatermarks: async (_h: unknown, marks: Record<string, number>) => {
        await tick()
        store.marks = { ...marks }
      },
      setGroupWatermarks: async (_h: unknown, groups: string[], xid8: number) => {
        const current = { ...store.marks }
        await tick()
        for (const g of groups) current[g] = xid8
        await tick()
        store.marks = current
      },
    },
  }
})

vi.mock('./idb', () => fake.idb)

const { dropGroupWatermarks, recordGroupWatermarks } = await import('./warm')

const handle = { name: 'test' } as unknown as IdbHandle

describe('watermark writes', () => {
  it('keep a dropped group dropped when a set interleaves', async () => {
    fake.store.marks = { 'workspace:6': 5, 'ticket:3': 5 }
    await Promise.all([
      recordGroupWatermarks(handle, ['workspace:6'], 10),
      dropGroupWatermarks(handle, ['ticket:3']),
      recordGroupWatermarks(handle, ['project:2'], 11),
    ])
    expect(fake.store.marks).toEqual({ 'workspace:6': 10, 'project:2': 11 })
  })

  it('run in the order they were asked for', async () => {
    fake.store.marks = {}
    await Promise.all([
      recordGroupWatermarks(handle, ['ticket:3'], 10),
      dropGroupWatermarks(handle, ['ticket:3']),
    ])
    expect(fake.store.marks).toEqual({})
  })

  it('carry on after one fails', async () => {
    fake.store.marks = { 'ticket:3': 5 }
    const failing = vi.spyOn(fake.idb, 'setGroupWatermarks').mockRejectedValueOnce(new Error('quota'))
    const set = recordGroupWatermarks(handle, ['workspace:6'], 10)
    const drop = dropGroupWatermarks(handle, ['ticket:3'])
    await expect(set).rejects.toThrow('quota')
    await drop
    expect(fake.store.marks).toEqual({})
    failing.mockRestore()
  })
})
