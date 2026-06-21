<script setup lang="ts">
import { ref } from 'vue'
import { Eye, EyeOff, Loader2, LogOut } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { useAuthStore } from '@/stores/auth'
import AdvancedDisclosure from './AdvancedDisclosure.vue'

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
  <Card>
    <CardHeader>
      <CardTitle>{{ t('settings.account_title') }}</CardTitle>
      <CardDescription>{{ t('settings.account_desc') }}</CardDescription>
    </CardHeader>
    <CardContent class="space-y-4">
      <div v-if="authStore.user" class="space-y-3">
        <div class="rounded-lg border border-green-500/20 bg-green-500/10 p-3 text-sm text-green-600 dark:text-green-400">
          {{ t('auth.authenticated_as') }} <span class="font-semibold">{{ authStore.user.username }}</span>
        </div>
        <Button variant="outline" class="gap-2" @click="authStore.logout">
          <LogOut class="h-4 w-4" />
          {{ t('general.logout') }}
        </Button>
      </div>

      <div v-else class="space-y-4">
        <Button
          @click="handleAutoDetect"
          :disabled="authStore.loading"
          size="lg"
          class="w-full gap-2"
        >
          <Loader2 v-if="authStore.loading" class="h-4 w-4 animate-spin" />
          {{ t('auth.auto_detect') }}
        </Button>

        <AdvancedDisclosure
          :title="t('settings.advanced_login_method')"
          :description="t('settings.advanced_login_desc')"
        >
          <div class="space-y-2">
            <div class="flex gap-2">
              <div class="relative flex-1">
                <Input
                  id="token"
                  v-model="manualToken"
                  :type="showToken ? 'text' : 'password'"
                  :placeholder="t('auth.enter_token')"
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  class="absolute right-0 top-0 h-full px-3 text-muted-foreground hover:text-foreground"
                  @click="showToken = !showToken"
                >
                  <Eye v-if="!showToken" class="h-4 w-4" />
                  <EyeOff v-else class="h-4 w-4" />
                </Button>
              </div>
              <Button @click="handleManualLogin" :disabled="authStore.loading || !manualToken">
                {{ t('auth.login') }}
              </Button>
            </div>
            <p class="text-xs text-muted-foreground">{{ t('settings.token_storage_note') }}</p>
            <p v-if="authStore.error" class="text-xs text-destructive">
              {{ authStore.error }}
            </p>
          </div>
        </AdvancedDisclosure>
      </div>
    </CardContent>
  </Card>
</template>
