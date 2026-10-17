import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getVirtualCurrencyBalance, getVirtualCurrencyTotalRedeemed } from './tauri'

const invoke = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke, Channel: vi.fn() }))

beforeEach(() => invoke.mockReset())

describe.each([
  ['balance', 'get_virtual_currency_balance', getVirtualCurrencyBalance, 87040],
  ['total_redeemed', 'get_virtual_currency_total_redeemed', getVirtualCurrencyTotalRedeemed, 133520],
] as const)('Orbs %s API contract', (field, command, fetchAmount, amount) => {
  it('reads the field observed in the authenticated Discord response', async () => {
    invoke.mockResolvedValue({ [field]: amount })
    await expect(fetchAmount()).resolves.toBe(amount)
    expect(invoke).toHaveBeenCalledWith(command)
  })

  it('accepts a real zero', async () => {
    invoke.mockResolvedValue({ [field]: 0 })
    await expect(fetchAmount()).resolves.toBe(0)
  })

  it.each([null, {}, [], { [field]: '87040' }, { [field]: -1 }, { [field]: 1.5 }, { [field]: Infinity }, { [field]: Number.MAX_SAFE_INTEGER + 1 }])(
    'rejects malformed data instead of reporting a zero', async response => {
      invoke.mockResolvedValue(response)
      await expect(fetchAmount()).rejects.toThrow(`Invalid virtual currency ${field} response`)
    },
  )
})
