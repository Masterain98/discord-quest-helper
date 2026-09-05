import { describe, expect, it } from 'vitest'
import { getNitroOrbsClaim } from './nitroOrbsCountdown'

const now = new Date('2026-09-05T04:27:00.000Z')

describe('getNitroOrbsClaim', () => {
  it('uses an absolute reward timestamp without local timezone arithmetic', () => {
    const utc = getNitroOrbsClaim('2026-09-22T00:21:08.745Z', now)
    const pacific = getNitroOrbsClaim('2026-09-21T17:21:08.745-07:00', now)

    expect(utc).toEqual({ value: 17, unit: 'days' })
    expect(pacific).toEqual(utc)
  })

  it('switches to hours below 48 hours and minutes below 12 hours', () => {
    expect(getNitroOrbsClaim('2026-09-06T20:26:59.000Z', now)).toEqual({
      value: 40,
      unit: 'hours',
    })
    expect(getNitroOrbsClaim('2026-09-05T04:39:00.000Z', now)).toEqual({
      value: 12,
      unit: 'minutes',
    })
  })

  it('rejects missing, invalid, and expired timestamps', () => {
    expect(getNitroOrbsClaim(null, now)).toBeNull()
    expect(getNitroOrbsClaim('not-a-date', now)).toBeNull()
    expect(getNitroOrbsClaim('2026-09-05T04:26:59.000Z', now)).toBeNull()
  })
})
