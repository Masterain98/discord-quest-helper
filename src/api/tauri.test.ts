import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getProgramRewards } from './tauri'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({
  Channel: vi.fn(),
  invoke: mocks.invoke,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

describe('getProgramRewards', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('normalizes Discord’s keyed rewards response', async () => {
    mocks.invoke.mockResolvedValue({
      rewards: {
        NITRO: {
          next_reward_date: '2026-09-22T00:21:08.745Z',
          program_current_state: 'active',
        },
        '2': {
          reward_program: 'XBOX',
          next_reward_date: null,
        },
      },
    })

    await expect(getProgramRewards()).resolves.toEqual(expect.arrayContaining([
      {
        reward_program: 'NITRO',
        next_reward_date: '2026-09-22T00:21:08.745Z',
        program_current_state: 'active',
      },
      {
        reward_program: 'XBOX',
        next_reward_date: null,
      },
    ]))
    expect(mocks.invoke).toHaveBeenCalledWith('get_program_rewards')
  })

  it('preserves the official array response and Nitro enum value', async () => {
    mocks.invoke.mockResolvedValue({
      rewards: [
        {
          reward_program: 0,
          next_reward_date: '2026-09-22T00:21:08.745Z',
          program_current_state: 'active',
          total_countdown_duration_ms: 2592000000,
        },
      ],
    })

    await expect(getProgramRewards()).resolves.toEqual([
      {
        reward_program: 0,
        next_reward_date: '2026-09-22T00:21:08.745Z',
        program_current_state: 'active',
        total_countdown_duration_ms: 2592000000,
      },
    ])
  })
})
