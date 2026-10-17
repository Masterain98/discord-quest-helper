import { describe, expect, it } from 'vitest'
import type { Quest, QuestReward, QuestUserStatus } from '@/api/tauri'
import { formatQuestReward, getQuestGroupRewardViews } from './questRewards'

function reward(overrides: Partial<QuestReward>): QuestReward {
  return {
    type: 4,
    sku_id: 'sku',
    messages: { name: 'Reward' },
    ...overrides,
  }
}

describe('formatQuestReward', () => {
  it('shows Nitro multiplier when user has Nitro and premium Orbs exceed base', () => {
    const view = formatQuestReward(reward({ orb_quantity: 700, premium_orb_quantity: 840 }), null, 2)

    expect(view.kind).toBe('orbs')
    expect(view.amountText).toBe('700 -> 840 Orbs')
    expect(view.badgeText).toBe('Nitro 1.2x')
  })

  it('hides Nitro multiplier when user has no Nitro even if premium data exists', () => {
    const view = formatQuestReward(reward({ orb_quantity: 700, premium_orb_quantity: 840 }), null, 0)

    expect(view.amountText).toBe('700 Orbs')
    expect(view.badgeText).toBeNull()
  })

  it('hides Nitro multiplier when premium_type is not provided', () => {
    const view = formatQuestReward(reward({ orb_quantity: 700, premium_orb_quantity: 840 }), null)

    expect(view.amountText).toBe('700 Orbs')
    expect(view.badgeText).toBeNull()
  })

  it('shows only base Orbs when premium quantity is missing', () => {
    const view = formatQuestReward(reward({ orb_quantity: 700, premium_orb_quantity: null }), null, 2)

    expect(view.amountText).toBe('700 Orbs')
    expect(view.badgeText).toBeNull()
  })

  it('uses claimed Orbs before estimated reward text', () => {
    const status: QuestUserStatus = {
      claimed_at: '2026-06-16T00:00:00.000Z',
      orb_quantity_claimed: 840,
    }

    const view = formatQuestReward(reward({ orb_quantity: 700, premium_orb_quantity: 840 }), status, 2)

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

describe('getQuestGroupRewardViews', () => {
  function member(id: string, rewards: QuestReward[], claimed = false): Quest {
    return {
      id,
      config: { messages: { quest_name: 'Group' }, rewards_config: { rewards } },
      user_status: claimed ? { claimed_at: '2026-10-10' } : null,
    }
  }

  it('adds different Orbs amounts and uses the base when a member has no Nitro bonus', () => {
    const members = [
      member('first', [reward({ orb_quantity: 100, premium_orb_quantity: 150 })]),
      member('second', [reward({ orb_quantity: 300, sku_id: 'other-orbs' })]),
    ]
    const before = JSON.stringify(members)
    expect(getQuestGroupRewardViews(members, 2).map(view => view.amountText)).toEqual(['400 -> 450 Orbs'])
    expect(getQuestGroupRewardViews(members, 0).map(view => view.amountText)).toEqual(['400 Orbs'])
    expect(JSON.stringify(members)).toBe(before)
  })

  it('includes rewards present only on later members and sums each non-Orbs reward separately', () => {
    const item = reward({ type: 2, sku_id: 'item', messages: { name: 'Item' }, quantity: 2 })
    const collectible = reward({ type: 3, sku_id: 'decoration', messages: { name: 'Decoration' } })
    const views = getQuestGroupRewardViews([
      member('first', [item]),
      member('second', [{ ...item, quantity: 3 }, collectible]),
      member('third', [collectible]),
    ])
    expect(views.map(view => view.amountText)).toEqual(['Item x5', 'Decoration x2'])
  })

  it('counts the supplied full group including claimed members, and handles empty rewards', () => {
    const members = [
      member('visible', [reward({ orb_quantity: 100 })]),
      member('claimed', [reward({ orb_quantity: 200 })], true),
      member('hidden', [reward({ orb_quantity: 300 })]),
      member('empty', []),
    ]
    expect(getQuestGroupRewardViews(members).map(view => view.amountText)).toEqual(['600 Orbs'])
    expect(getQuestGroupRewardViews([])).toEqual([])
    expect(getQuestGroupRewardViews([member('empty', [])])).toEqual([])
  })
})
