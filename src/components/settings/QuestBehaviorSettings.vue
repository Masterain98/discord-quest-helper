<script setup lang="ts">
import { AlertTriangle, Bot, Gamepad2, MonitorPlay, Wifi } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { Badge } from '@/components/ui/badge'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useQuestsStore } from '@/stores/quests'
import { navigateToTab } from '@/utils/navigate'
import AdvancedDisclosure from './AdvancedDisclosure.vue'
import SettingRow from './SettingRow.vue'
import SettingsSectionCard from './SettingsSectionCard.vue'
import SettingsStatusPanel from './SettingsStatusPanel.vue'
import { cn } from '@/lib/utils'
import { settingToneClass } from './settingTones'

const { t } = useI18n()
const questsStore = useQuestsStore()
</script>

<template>
  <SettingsSectionCard
    :title="t('settings.quest_behavior_title')"
    :description="t('settings.quest_behavior_desc')"
    :icon="Gamepad2"
    tone="violet"
    content-class="space-y-6"
  >
      <div class="space-y-3">
        <Label>{{ t('settings.game_quest_mode') }}</Label>
        <div class="grid gap-3 md:grid-cols-2">
          <button
            @click="questsStore.gameQuestMode = 'simulate'"
            :class="cn(
              'rounded-lg border-2 p-4 text-left transition-all hover:-translate-y-0.5 hover:shadow-sm',
              questsStore.gameQuestMode === 'simulate'
                ? 'border-primary bg-primary/10 text-primary'
                : 'border-border bg-card hover:border-primary/40 hover:bg-primary/5',
            )"
          >
            <div class="flex items-start justify-between gap-3">
              <div class="flex min-w-0 gap-3">
                <div :class="cn('flex h-10 w-10 shrink-0 items-center justify-center rounded-md', settingToneClass.primary.icon)">
                  <Bot class="h-5 w-5" />
                </div>
                <div class="min-w-0">
                  <div class="font-semibold">{{ t('settings.game_mode_simulate') }}</div>
                  <div class="mt-1 text-xs text-muted-foreground">{{ t('settings.game_mode_simulate_desc') }}</div>
                </div>
              </div>
              <Badge
                v-if="questsStore.gameQuestMode === 'simulate'"
                variant="outline"
                :class="cn('shrink-0 text-[10px]', settingToneClass.primary.badge)"
              >
                {{ t('settings.status_ok') }}
              </Badge>
            </div>
          </button>

          <button
            @click="questsStore.cdpAvailable ? questsStore.gameQuestMode = 'cdp' : navigateToTab('settings', 'discord_integration')"
            :class="cn(
              'rounded-lg border-2 p-4 text-left transition-all hover:-translate-y-0.5 hover:shadow-sm',
              questsStore.gameQuestMode === 'cdp'
                ? 'border-emerald-500 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300'
                : questsStore.cdpAvailable
                  ? 'border-border bg-card hover:border-emerald-500/40 hover:bg-emerald-500/5'
                  : 'border-border bg-card opacity-80 hover:border-amber-500/40 hover:bg-amber-500/5',
            )"
          >
            <div class="flex items-start justify-between gap-3">
              <div class="flex min-w-0 gap-3">
                <div :class="cn('flex h-10 w-10 shrink-0 items-center justify-center rounded-md', settingToneClass[questsStore.cdpAvailable ? 'success' : 'warning'].icon)">
                  <MonitorPlay v-if="questsStore.cdpAvailable" class="h-5 w-5" />
                  <Wifi v-else class="h-5 w-5" />
                </div>
                <div class="min-w-0">
                  <div class="font-semibold">{{ t('settings.game_mode_cdp') }}</div>
                  <div class="mt-1 text-xs text-muted-foreground">
                    <template v-if="questsStore.cdpAvailable">{{ t('settings.game_mode_cdp_desc') }}</template>
                    <template v-else>{{ t('settings.game_mode_cdp_unavailable') }}</template>
                  </div>
                </div>
              </div>
              <Badge
                variant="outline"
                :class="cn('shrink-0 text-[10px]', settingToneClass[questsStore.cdpAvailable ? 'success' : 'warning'].badge)"
              >
                {{ questsStore.cdpAvailable ? t('settings.game_mode_cdp_connected') : t('settings.status_attention') }}
              </Badge>
            </div>
          </button>
        </div>
      </div>

      <SettingsStatusPanel v-if="questsStore.gameQuestMode === 'cdp'" tone="info" :icon="AlertTriangle">
        {{ t('settings.video_config_cdp_notice') }}
      </SettingsStatusPanel>

      <div class="space-y-3">
        <Label>{{ t('settings.video_task_settings') }}</Label>
        <div class="rounded-lg border px-4">
          <SettingRow
            :label="t('settings.completion_speed')"
            :description="t('settings.speed_hint')"
          >
            <div class="flex items-center gap-3">
              <input
                type="range"
                v-model.number="questsStore.speedMultiplier"
                min="0.1"
                max="2.0"
                step="0.1"
                :disabled="questsStore.gameQuestMode === 'cdp'"
                :aria-label="t('settings.completion_speed')"
                class="w-48 accent-primary disabled:opacity-50"
              />
              <Badge variant="outline" :class="settingToneClass.primary.badge">
                {{ questsStore.speedMultiplier }}x
              </Badge>
            </div>
          </SettingRow>

          <SettingRow
            :label="t('settings.request_interval')"
            :description="t('settings.interval_hint')"
          >
            <div class="flex items-center gap-3">
              <input
                type="range"
                v-model.number="questsStore.heartbeatInterval"
                min="10"
                max="30"
                step="1"
                :disabled="questsStore.gameQuestMode === 'cdp'"
                :aria-label="t('settings.request_interval')"
                class="w-48 accent-primary disabled:opacity-50"
              />
              <Badge variant="outline" :class="settingToneClass.primary.badge">
                {{ questsStore.heartbeatInterval }}s
              </Badge>
            </div>
          </SettingRow>
        </div>
      </div>

      <div class="space-y-3">
        <Label>{{ t('settings.general_task_settings') }}</Label>
        <div class="rounded-lg border px-4">
          <SettingRow
            :label="t('settings.game_polling_interval')"
            :description="t('settings.game_polling_hint')"
          >
            <div class="flex items-center gap-3">
              <input
                type="range"
                v-model.number="questsStore.gamePollingInterval"
                min="30"
                max="300"
                step="1"
                :aria-label="t('settings.game_polling_interval')"
                class="w-48 accent-primary"
              />
              <Badge variant="outline" :class="settingToneClass.violet.badge">
                {{ questsStore.gamePollingInterval }}s
              </Badge>
            </div>
          </SettingRow>
        </div>
      </div>

      <AdvancedDisclosure
        :title="t('settings.activity_timing_advanced')"
        :description="t('settings.activity_timing_advanced_desc')"
        tone="warning"
        default-open
      >
        <SettingsStatusPanel v-if="!questsStore.cdpAvailable" tone="warning" :icon="AlertTriangle" class="mb-4">
          {{ t('settings.activity_cdp_required') }}
        </SettingsStatusPanel>
        <div class="grid gap-4 md:grid-cols-2">
          <div class="space-y-2">
            <Label>{{ t('settings.activity_checkpoint_min') }}</Label>
            <div class="flex items-center gap-2">
              <Input
                type="number"
                v-model.number="questsStore.activityCheckpointMin"
                min="30"
                :max="questsStore.activityCheckpointMax"
                :aria-label="t('settings.activity_checkpoint_min')"
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
                :aria-label="t('settings.activity_checkpoint_max')"
                class="w-24"
              />
              <span class="text-sm text-muted-foreground">{{ t('settings.activity_checkpoint_unit') }}</span>
            </div>
          </div>
        </div>
      </AdvancedDisclosure>
  </SettingsSectionCard>
</template>
