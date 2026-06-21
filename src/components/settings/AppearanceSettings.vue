<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { useQuestsStore } from '@/stores/quests'

const { t } = useI18n()
const questsStore = useQuestsStore()
</script>

<template>
  <Card>
    <CardHeader>
      <CardTitle>{{ t('settings.display') }}</CardTitle>
      <CardDescription>{{ t('settings.display_desc') }}</CardDescription>
    </CardHeader>
    <CardContent class="space-y-4">
      <label class="flex items-start gap-3 rounded-lg border border-border p-3">
        <input
          v-model="questsStore.showOrbsBalance"
          type="checkbox"
          class="mt-1 h-4 w-4 accent-primary"
        />
        <span class="min-w-0 flex-1">
          <span class="flex items-center gap-2 text-sm font-medium">
            <img src="/icons/orbs.png" alt="" class="h-5 w-5 object-contain" />
            {{ t('settings.show_orbs_balance') }}
          </span>
          <span class="mt-1 block text-xs text-muted-foreground">
            {{ t('settings.show_orbs_balance_desc') }}
          </span>
        </span>
      </label>
    </CardContent>
  </Card>
</template>
