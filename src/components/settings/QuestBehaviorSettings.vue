<script setup lang="ts">
import { AlertTriangle } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useQuestsStore } from '@/stores/quests'
import AdvancedDisclosure from './AdvancedDisclosure.vue'
import SettingRow from './SettingRow.vue'

const { t } = useI18n()
const questsStore = useQuestsStore()
</script>

<template>
  <Card>
    <CardHeader>
      <CardTitle>{{ t('settings.quest_behavior_title') }}</CardTitle>
      <CardDescription>{{ t('settings.quest_behavior_desc') }}</CardDescription>
    </CardHeader>
    <CardContent class="space-y-6">
      <div class="space-y-3">
        <Label>{{ t('settings.game_quest_mode') }}</Label>
        <div class="grid gap-3 md:grid-cols-2">
          <button
            @click="questsStore.gameQuestMode = 'simulate'"
            :class="[
              'rounded-lg border-2 p-4 text-left transition-all',
              questsStore.gameQuestMode === 'simulate'
                ? 'border-primary bg-primary/5'
                : 'border-border hover:border-primary/50',
            ]"
          >
            <div class="font-medium">{{ t('settings.game_mode_simulate') }}</div>
            <div class="mt-1 text-xs text-muted-foreground">{{ t('settings.game_mode_simulate_desc') }}</div>
          </button>

          <button
            @click="questsStore.cdpAvailable ? questsStore.gameQuestMode = 'cdp' : null"
            :disabled="!questsStore.cdpAvailable"
            :class="[
              'rounded-lg border-2 p-4 text-left transition-all',
              questsStore.gameQuestMode === 'cdp'
                ? 'border-green-500 bg-green-500/5'
                : questsStore.cdpAvailable
                  ? 'border-border hover:border-green-500/50'
                  : 'cursor-not-allowed border-border opacity-50',
            ]"
          >
            <div class="flex items-center gap-2 font-medium">
              {{ t('settings.game_mode_cdp') }}
              <Badge v-if="questsStore.gameQuestMode === 'cdp'" variant="outline" class="border-green-500/50 text-[10px] text-green-500">
                {{ t('settings.game_mode_cdp_connected') }}
              </Badge>
            </div>
            <div class="mt-1 text-xs text-muted-foreground">
              <template v-if="questsStore.cdpAvailable">{{ t('settings.game_mode_cdp_desc') }}</template>
              <template v-else>{{ t('settings.game_mode_cdp_unavailable') }}</template>
            </div>
          </button>
        </div>
      </div>

      <div v-if="questsStore.gameQuestMode === 'cdp'" class="flex items-start gap-2.5 rounded-md border border-blue-500/30 bg-blue-500/10 px-4 py-3 text-sm text-blue-500">
        <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" />
        <span>{{ t('settings.video_config_cdp_notice') }}</span>
      </div>

      <div class="rounded-lg border px-4">
        <SettingRow
          :label="`${t('settings.completion_speed')} (${questsStore.speedMultiplier}x)`"
          :description="t('settings.speed_hint')"
        >
          <input
            type="range"
            v-model.number="questsStore.speedMultiplier"
            min="0.1"
            max="2.0"
            step="0.1"
            :disabled="questsStore.gameQuestMode === 'cdp'"
            class="w-48 accent-primary disabled:opacity-50"
          />
        </SettingRow>

        <SettingRow
          :label="`${t('settings.request_interval')} (${questsStore.heartbeatInterval}s)`"
          :description="t('settings.interval_hint')"
        >
          <input
            type="range"
            v-model.number="questsStore.heartbeatInterval"
            min="10"
            max="30"
            step="1"
            :disabled="questsStore.gameQuestMode === 'cdp'"
            class="w-48 accent-primary disabled:opacity-50"
          />
        </SettingRow>

        <SettingRow
          :label="`${t('settings.game_polling_interval')} (${questsStore.gamePollingInterval}s)`"
          :description="t('settings.game_polling_hint')"
        >
          <input
            type="range"
            v-model.number="questsStore.gamePollingInterval"
            min="30"
            max="300"
            step="1"
            class="w-48 accent-primary"
          />
        </SettingRow>
      </div>

      <AdvancedDisclosure
        :title="t('settings.activity_timing_advanced')"
        :description="t('settings.activity_timing_advanced_desc')"
        default-open
      >
        <div v-if="!questsStore.cdpAvailable" class="mb-4 flex items-start gap-2.5 rounded-md border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-sm text-amber-500">
          <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" />
          <span>{{ t('settings.activity_cdp_required') }}</span>
        </div>
        <div class="grid gap-4 md:grid-cols-2">
          <div class="space-y-2">
            <Label>{{ t('settings.activity_checkpoint_min') }}</Label>
            <div class="flex items-center gap-2">
              <Input
                type="number"
                v-model.number="questsStore.activityCheckpointMin"
                min="30"
                :max="questsStore.activityCheckpointMax"
                class="w-24"
              />
              <span class="text-sm text-muted-foreground">{{ t('settings.activity_checkpoint_unit') }}</span>
            </div>
          </div>

          <div class="space-y-2">
            <Label>{{ t('settings.activity_checkpoint_max') }}</Label>
            <div class="flex items-center gap-2">
              <Input
                type="number"
                v-model.number="questsStore.activityCheckpointMax"
                :min="questsStore.activityCheckpointMin"
                max="900"
                class="w-24"
              />
              <span class="text-sm text-muted-foreground">{{ t('settings.activity_checkpoint_unit') }}</span>
            </div>
          </div>
        </div>
      </AdvancedDisclosure>
    </CardContent>
  </Card>
</template>
