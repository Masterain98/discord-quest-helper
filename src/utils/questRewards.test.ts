import { describe, expect, it } from 'vitest'
import type { QuestReward, QuestUserStatus } from '@/api/tauri'
import { formatQuestReward } from './questRewards'

function reward(overrides: Partial<QuestReward>): QuestReward {
  return {
    type: 4,
    sku_id: 'sku',
    messages: { name: 'Reward' },
    ...overrides,
  }
}

describe('formatQuestReward', () => {
  it('shows Nitro multiplier when premium Orbs exceed base Orbs', () => {
    const view = formatQuestReward(reward({ orb_quantity: 700, premium_orb_quantity: 840 }), null)

    expect(view.kind).toBe('orbs')
    expect(view.amountText).toBe('700 -> 840 Orbs')
    expect(view.badgeText).toBe('Nitro 1.2x')
  })

  it('shows only base Orbs when premium quantity is missing', () => {
    const view = formatQuestReward(reward({ orb_quantity: 700, premium_orb_quantity: null }), null)

    expect(view.amountText).toBe('700 Orbs')
    expect(view.badgeText).toBeNull()
  })

  it('uses claimed Orbs before estimated reward text', () => {
    const status: QuestUserStatus = {
      claimed_at: '2026-06-16T00:00:00.000Z',
      orb_quantity_claimed: 840,
    }

    const view = formatQuestReward(reward({ orb_quantity: 700, premium_orb_quantity: 840 }), status)

    expect(view.amountText).toBe('Claimed 840 Orbs')
  })

  it('recognizes Orbs by structured type even when the name does not contain Orb', () => {
    const view = formatQuestReward(reward({
      type: 4,
      messages: { name: 'Premium currency' },
      orb_quantity: 200,
    }), null)

    expect(view.kind).toBe('orbs')
    expect(view.amountText).toBe('200 Orbs')
  })
})
