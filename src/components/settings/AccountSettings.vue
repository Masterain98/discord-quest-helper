<script setup lang="ts">
import { ref } from 'vue'
import { CheckCircle2, Eye, EyeOff, Loader2, LogOut, ShieldCheck, UserRound } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useAuthStore } from '@/stores/auth'
import AdvancedDisclosure from './AdvancedDisclosure.vue'
import SettingsSectionCard from './SettingsSectionCard.vue'
import SettingsStatusPanel from './SettingsStatusPanel.vue'
import { settingToneClass } from './settingTones'

const { t } = useI18n()
const authStore = useAuthStore()
const emit = defineEmits<{
  navigateToHome: []
}>()

const manualToken = ref('')
const showToken = ref(false)

async function handleAutoDetect() {
  await authStore.tryAutoDetect()
  if (authStore.detectedAccounts.length > 0) {
    emit('navigateToHome')
  }
}

async function handleManualLogin() {
  if (manualToken.value) {
    await authStore.loginWithToken(manualToken.value)
    manualToken.value = ''
  }
}
</script>

<template>
  <SettingsSectionCard
    :title="t('settings.account_title')"
    :description="t('settings.account_desc')"
    :icon="authStore.user ? ShieldCheck : UserRound"
    :tone="authStore.user ? 'success' : 'primary'"
  >
      <div v-if="authStore.user" class="space-y-3">
        <SettingsStatusPanel tone="success" :icon="CheckCircle2">
          {{ t('auth.authenticated_as') }} <span class="font-semibold">{{ authStore.user.username }}</span>
        </SettingsStatusPanel>
        <Button variant="outline" :class="['gap-2', settingToneClass.danger.buttonSoft]" @click="authStore.logout">
          <LogOut class="h-4 w-4" />
          {{ t('general.logout') }}
        </Button>
      </div>

      <div v-else class="space-y-4">
        <Button
          @click="handleAutoDetect"
          :disabled="authStore.loading"
          size="lg"
          class="w-full gap-2 shadow-sm"
        >
          <Loader2 v-if="authStore.loading" class="h-4 w-4 animate-spin" />
          {{ t('auth.auto_detect') }}
        </Button>

        <AdvancedDisclosure
          :title="t('settings.advanced_login_method')"
          :description="t('settings.advanced_login_desc')"
          tone="warning"
        >
          <div class="space-y-2">
            <div class="flex gap-2">
              <div class="relative flex-1">
                <Input
                  id="token"
                  v-model="manualToken"
                  :type="showToken ? 'text' : 'password'"
                  :placeholder="t('auth.enter_token')"
                  :aria-label="t('auth.enter_token')"
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  :aria-label="showToken ? 'Hide token' : 'Show token'"
                  :aria-pressed="showToken"
                  class="absolute right-0 top-0 h-full px-3 text-muted-foreground hover:text-foreground"
                  @click="showToken = !showToken"
                >
                  <Eye v-if="!showToken" class="h-4 w-4" />
                  <EyeOff v-else class="h-4 w-4" />
                </Button>
              </div>
              <Button
                variant="outline"
                :class="settingToneClass.primary.buttonSoft"
                @click="handleManualLogin"
                :disabled="authStore.loading || !manualToken"
              >
                {{ t('auth.login') }}
              </Button>
            </div>
            <p class="text-xs text-muted-foreground">{{ t('settings.token_storage_note') }}</p>
            <SettingsStatusPanel v-if="authStore.error" tone="danger">
              {{ authStore.error }}
            </SettingsStatusPanel>
          </div>
        </AdvancedDisclosure>
      </div>
  </SettingsSectionCard>
</template>
