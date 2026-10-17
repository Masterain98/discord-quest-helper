<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Quest } from '@/api/tauri'
import { useQuestsStore } from '@/stores/quests'
import { canAcceptGroupMember, canClaimGroupMember, canQueueGroupMember } from '@/utils/questGroups'
import { getQuestKind } from '@/utils/questTasks'
import { Button } from '@/components/ui/button'
import { Loader2 } from 'lucide-vue-next'

const props = defineProps<{ quests: Quest[], disabled: boolean, busy: boolean, executionDisabled: boolean }>()
defineEmits<{ accept: [], run: [], remove: [], stop: [], claim: [] }>()
const { t } = useI18n()
const store = useQuestsStore()
const accepts = computed(() => props.quests.filter(canAcceptGroupMember).length)
const claims = computed(() => props.quests.filter(canClaimGroupMember).length)
const ids = computed(() => new Set(props.quests.map(quest => quest.id)))
const pending = computed(() => store.upcomingQuests.filter(quest => ids.value.has(quest.id)).length)
const active = computed(() => !!store.activeQuestId && ids.value.has(store.activeQuestId))
const runnable = computed(() => props.quests.filter(quest => canQueueGroupMember(quest) && quest.id !== store.activeQuestId && !store.questQueue.some(item => item.id === quest.id)).length)
const manual = computed(() => props.quests.some(quest => getQuestKind(quest) === 'activity' && quest.user_status?.enrolled_at && !quest.user_status.completed_at))
</script>

<template>
  <div class="flex flex-wrap items-center justify-end gap-2" :aria-busy="busy || undefined">
    <Loader2 v-if="busy" class="h-4 w-4 animate-spin text-primary" :aria-label="t('general.loading')" />
    <Button v-if="accepts" class="pr-3" variant="outline" :disabled="disabled" @click="$emit('accept')">{{ t('quest_group.accept', { count: accepts }) }}</Button>
    <Button v-if="runnable" class="pr-3" :disabled="disabled || executionDisabled || store.queueEditingDisabled" @click="$emit('run')">{{ t(store.activeQuestId || store.isQueueRunning ? 'quest_group.queue' : 'quest_group.start', { count: runnable }) }}</Button>
    <Button v-else-if="pending && !store.activeQuestId && !store.isQueueRunning" class="pr-3" :disabled="disabled || executionDisabled || store.queueEditingDisabled" @click="$emit('run')">{{ t('quest_group.resume') }}</Button>
    <Button v-if="active" class="pr-3" variant="destructive" :disabled="disabled || store.queueEditingDisabled" @click="$emit('stop')">{{ t('quest_group.stop') }}</Button>
    <Button v-else-if="pending" class="pr-3" variant="outline" :disabled="disabled || store.queueEditingDisabled" @click="$emit('remove')">{{ t('quest_group.remove', { count: pending }) }}</Button>
    <Button v-if="claims" class="pr-3" variant="outline" :disabled="disabled" @click="$emit('claim')">{{ t('quest_group.claim', { count: claims }) }}</Button>
    <p v-if="manual" class="w-full text-xs text-muted-foreground">{{ t('quest_group.activity_manual') }}</p>
  </div>
</template>
