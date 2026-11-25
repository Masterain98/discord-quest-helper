import { computed, onScopeDispose, ref } from 'vue'
import type { Quest } from '@/api/tauri'
import { acceptQuest, navigateDiscordSpa } from '@/api/tauri'
import { useQuestsStore } from '@/stores/quests'
import { useToastStore } from '@/stores/toast'
import { canAcceptGroupMember, canClaimGroupMember, canQueueGroupMember } from '@/utils/questGroups'
import { discordIntegrationAction, manualRewardClaimNotice, openRewardQuest } from '@/utils/questRewardNavigation'

type Options = {
  blocked: () => boolean
  prepareExecutables: (quests: Quest[]) => Promise<Map<string, string>>
  t: (key: string, params?: Record<string, string | number>) => string
}

type GuideFeedback = {
  message: string
  action?: ReturnType<typeof discordIntegrationAction>
}

export function useQuestGroupActions(options: Options) {
  const store = useQuestsStore()
  const toast = useToastStore()
  const busyIds = ref(new Set<string>())
  const guide = ref<{ ids: string[], index: number } | null>(null)
  const guideBusy = ref(false)
  const guideFeedback = ref<GuideFeedback | null>(null)
  let revision = 0
  const busy = computed(() => busyIds.value.size > 0)
  const currentGuideQuest = computed(() => store.quests.find(q => q.id === guide.value?.ids[guide.value.index]))
  const current = (id: string) => store.quests.find(q => q.id === id)

  function lock(quests: Quest[]): number | null {
    if (busy.value || options.blocked()) return null
    busyIds.value = new Set(quests.map(q => q.id))
    return ++revision
  }
  function cancel() {
    revision++
    guide.value = null
    guideBusy.value = false
    guideFeedback.value = null
    busyIds.value = new Set()
  }
  function release(token: number) { if (token === revision) cancel() }
  function reportAccept(success: number, fail: number) {
    toast[fail ? 'warning' : 'success']({ title: options.t('quest_group.accept_result', { success, fail }) })
  }
  function reportError(error: unknown) {
    toast.error({ title: options.t('quest_group.operation_failed'), description: String(error) })
  }
  function reportGuideError(token: number, error: unknown) {
    if (token !== revision) return
    guideFeedback.value = { message: `${options.t('toast.failed_open_quest')}: ${String(error)}` }
  }

  async function accept(quests: Quest[]) {
    const token = lock(quests)
    if (token === null) return
    let success = 0; let fail = 0
    try {
      for (const id of busyIds.value) {
        if (token !== revision) return
        const quest = current(id)
        if (!quest || !canAcceptGroupMember(quest)) continue
        try {
          await acceptQuest(id)
          if (token !== revision) return
          store.updateQuestEnrollment(id, new Date().toISOString())
          success++
        } catch { fail++ }
        await new Promise(resolve => setTimeout(resolve, 500))
      }
      if (token === revision) reportAccept(success, fail)
    } finally { release(token) }
  }

  async function run(quests: Quest[]) {
    if (store.queueEditingDisabled) return
    const token = lock(quests)
    if (token === null) return
    try {
      const candidates = quests.map(q => current(q.id)).filter((q): q is Quest => !!q && canQueueGroupMember(q) && q.id !== store.activeQuestId)
      const newMembers = candidates.filter(q => !store.questQueue.some(item => item.id === q.id))
      const selections = await options.prepareExecutables(newMembers)
      if (token !== revision || options.blocked() || store.queueEditingDisabled) return
      store.addGroupToQueue(newMembers.map(q => q.id), selections)
      if (!store.activeQuestId && candidates.length) await store.startQueue()
    } catch (error) {
      if (token === revision && !(error instanceof Error && error.message === 'User cancelled')) reportError(error)
    } finally { release(token) }
  }

  async function stop(quests: Quest[]) {
    if (store.queueEditingDisabled) return
    const token = lock(quests)
    if (token === null) return
    try {
      if (!await store.stopGroup(quests.map(q => q.id)) && token === revision) reportError(store.error ?? options.t('quest_group.operation_failed'))
    } catch (error) { if (token === revision) reportError(error) }
    finally { release(token) }
  }

  function remove(quests: Quest[]) {
    if (!busy.value && !options.blocked()) store.removeQueuedQuests(quests.map(q => q.id))
  }

  async function openGuideMember(token: number) {
    const state = guide.value
    if (!state || token !== revision) return
    // Members already claimed while the guide was open need no further navigation.
    while (state.index < state.ids.length && current(state.ids[state.index])?.user_status?.claimed_at) state.index++
    if (state.index >= state.ids.length) { release(token); return }
    const result = await openRewardQuest(state.ids[state.index], {
      cdpAvailable: store.cdpAvailable,
      cdpPort: store.cdpPort,
      navigate: navigateDiscordSpa,
    })
    if (token !== revision) return
    if (result === 'manual') {
      guideFeedback.value = {
        message: options.t('quest_group.guide_disconnected'),
        action: discordIntegrationAction(options.t),
      }
    }
  }

  async function claim(quests: Quest[]) {
    const token = lock(quests)
    if (token === null) return
    let keepGuide = false
    try {
      const members = quests.map(q => current(q.id)).filter((q): q is Quest => !!q && canClaimGroupMember(q))
      if (!members.length) return
      if (!store.cdpAvailable) {
        toast.info(manualRewardClaimNotice(options.t))
        return
      }
      guide.value = { ids: members.map(q => q.id), index: 0 }
      keepGuide = true
      guideBusy.value = true
      try { await openGuideMember(token) }
      catch (error) { reportGuideError(token, error) }
      finally { if (token === revision) guideBusy.value = false }
    } finally { if (!keepGuide) release(token) }
  }

  async function checkAndContinue() {
    if (!guide.value || guideBusy.value) return
    const token = revision
    const id = guide.value.ids[guide.value.index]
    guideBusy.value = true
    guideFeedback.value = null
    try {
      const refreshed = await store.fetchQuests(true, true)
      if (token !== revision || !guide.value) return
      if (!refreshed) {
        guideFeedback.value = { message: store.error ?? options.t('quest_group.operation_failed') }
        return
      }
      if (!current(id)?.user_status?.claimed_at) {
        guideFeedback.value = { message: options.t('quest_group.guide_unclaimed') }
        return
      }
      guide.value.index++
      await openGuideMember(token)
    } catch (error) { reportGuideError(token, error) }
    finally { if (token === revision) guideBusy.value = false }
  }

  async function reopenGuideMember() {
    if (!guide.value || guideBusy.value) return
    const token = revision
    guideBusy.value = true
    guideFeedback.value = null
    try { await openGuideMember(token) }
    catch (error) { reportGuideError(token, error) }
    finally { if (token === revision) guideBusy.value = false }
  }

  onScopeDispose(cancel)
  return { busyIds, busy, guide, guideBusy, guideFeedback, currentGuideQuest, accept, run, stop, remove, claim, cancel, checkAndContinue, reopenGuideMember }
}
