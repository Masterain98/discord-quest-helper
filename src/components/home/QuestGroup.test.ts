import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createSSRApp, h } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { createI18n } from 'vue-i18n'
import type { Quest } from '@/api/tauri'
import en from '@/locales/en.json'
import { groupQuests } from '@/utils/questGroups'
import QuestGroup from './QuestGroup.vue'

const store = vi.hoisted(() => ({
  quests: [] as Quest[], activeQuestId: null, activeQuestProgress: 0,
}))
vi.mock('@/stores/quests', () => ({ useQuestsStore: () => store }))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ user: null }) }))
vi.mock('/icons/orbs.png', () => ({ default: '/icons/orbs.png' }))

function quest(id: string, value = 0): Quest {
  return {
    id,
    config: {
      messages: { quest_name: 'Clean Crew', game_title: 'Roblox' },
      task_config_v2: { tasks: { PLAY_ON_DESKTOP: { target: 900 } } },
      rewards_config: { rewards: [{ type: 4, sku_id: 'orbs', orb_quantity: 100, messages: { name: 'Orbs' } }] },
    },
    user_status: { enrolled_at: '2026-01-01', progress: { PLAY_ON_DESKTOP: { value } } },
  }
}

function render(visible: Quest[], compact = false) {
  const app = createSSRApp({
    render: () => h('main', groupQuests(visible).map(group => h(QuestGroup, {
      key: group.key, group, expanded: true, compact,
    }, {
      default: ({ quest }: { quest: Quest }) => h('article', { 'data-quest-id': quest.id }, quest.id),
    }))),
  })
  app.use(createI18n({ legacy: false, locale: 'en', messages: { en } }))
  return renderToString(app)
}

beforeEach(() => { store.quests = [] })

describe('quest stack rendering', () => {
  it.each([false, true])('excludes completed account members from summaries (compact: %s)', async compact => {
    const a = quest('a', 90); const b = quest('b', 180); const hidden = quest('hidden', 540)
    const claimable = quest('claimable'); claimable.user_status!.completed_at = 'confirmed'
    const claimed = quest('claimed'); claimed.user_status!.claimed_at = 'claimed'
    claimable.config.expires_at = claimed.config.expires_at = '2020-01-01'
    store.quests = [a, b, hidden, claimable, claimed]

    const html = await render([a, b], compact)

    expect(html).toContain(`Group · ${compact ? 2 : 3}`)
    expect(html).toContain('Group rewards total: 300 Orbs')
    expect(html).toContain(en.quest.no_expiry)
    expect(html).not.toContain('data-quest-id="claimable"')
    expect(html).not.toContain('data-quest-id="claimed"')
    expect(html).not.toContain('data-quest-id="hidden"')
    if (!compact) {
      expect(html).toContain('Accepted 3/3')
      expect(html).toContain('Completed 0/3')
      expect(html).toContain('Visible 2/3')
      expect(html).toContain('aria-valuenow="30"')
    }
  })

  it('renders one remaining quest and two claimed quests without a stack after refresh', async () => {
    const members = [quest('a'), quest('b'), quest('c')]
    store.quests = members
    expect(await render(members)).toContain('quest-group-stack')

    for (const member of members.slice(0, 2)) {
      member.user_status!.completed_at = 'confirmed'
      member.user_status!.claimed_at = 'claimed'
    }
    const html = await render(members)
    expect(html).not.toContain('quest-group-stack')
    for (const member of members) expect(html).toContain(`data-quest-id="${member.id}"`)
  })
})
