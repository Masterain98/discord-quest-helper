<template>
  <div class="user-status">
    <div v-if="authStore.loading" class="flex items-center gap-2 text-sm text-muted-foreground">
      <Loader2 class="w-4 h-4 animate-spin" />
      <span>Loading...</span>
    </div>

    <div v-else-if="authStore.user" class="flex items-center gap-4">
      <div class="flex items-center gap-3">
        <Avatar>
          <AvatarImage 
            v-if="authStore.user.avatar" 
            :src="`https://cdn.discordapp.com/avatars/${authStore.user.id}/${authStore.user.avatar}.png?size=128`" 
            :alt="authStore.user.username" 
          />
          <AvatarFallback>{{ authStore.user.username[0].toUpperCase() }}</AvatarFallback>
        </Avatar>
        <div class="hidden md:block select-none">
          <p class="text-sm font-medium leading-none flex items-center gap-1.5">
            {{ authStore.user.global_name || authStore.user.username }}
            <span
              v-if="nitroBadge"
              :class="['text-[10px] font-semibold leading-none px-1.5 py-0.5 rounded-full border', nitroBadge.class]"
            >
              {{ nitroBadge.label }}
            </span>
          </p>
          <p class="text-xs text-muted-foreground">{{ authStore.user.username }}</p>
        </div>
      </div>
      
      <Button variant="destructive" size="sm" @click="authStore.logout">
        {{ t('general.logout') }}
      </Button>
    </div>

    <div v-else class="flex flex-col md:flex-row items-center gap-3">
      <div v-if="!showManualInput" class="flex items-center gap-2">
         <span class="text-sm text-muted-foreground mr-2 hidden md:inline">Not logged in</span>
         <Button @click="handleAutoDetect" :disabled="authStore.loading" variant="default" size="sm">
            Auto Detect Token
         </Button>
         <Button @click="showManualInput = true" variant="outline" size="sm">
            Manual Input
         </Button>
      </div>
      
      <div v-if="showManualInput" class="flex items-center gap-2 animate-in slide-in-from-right-5 fade-in duration-300">
        <div class="relative">
           <Input 
             v-model="manualToken" 
             type="password" 
             placeholder="Paste Token" 
             class="w-[200px] h-9"
           />
        </div>
        <Button @click="handleManualLogin" :disabled="authStore.loading || !manualToken" size="sm">
          Login
        </Button>
        <Button @click="showManualInput = false" variant="ghost" size="icon" class="h-9 w-9">
           <X class="w-4 h-4" />
        </Button>
      </div>
      
      <p v-if="authStore.error" class="text-xs text-destructive absolute -bottom-5 right-0">{{ authStore.error }}</p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Loader2, X } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const authStore = useAuthStore()
const questsStore = useQuestsStore()

const nitroBadge = computed(() => {
  const pt = authStore.user?.premium_type
  if (!pt || pt === 0) return null
  if (pt === 1) return { label: t('user.nitro_classic'), class: 'border-sky-400/60 bg-sky-500/10 text-sky-600 dark:text-sky-400' }
  if (pt === 2) return { label: t('user.nitro'), class: 'border-violet-400/60 bg-violet-500/10 text-violet-600 dark:text-violet-400' }
  if (pt === 3) return { label: t('user.nitro_basic'), class: 'border-indigo-400/60 bg-indigo-500/10 text-indigo-600 dark:text-indigo-400' }
  return null
})
const showManualInput = ref(false)
const manualToken = ref('')

async function handleAutoDetect() {
  const success = await authStore.tryAutoDetect()
  if (success) {
    await questsStore.fetchQuests()
  }
}

async function handleManualLogin() {
  if (manualToken.value) {
    const success = await authStore.loginWithToken(manualToken.value)
    if (success) {
      showManualInput.value = false
      manualToken.value = ''
      await questsStore.fetchQuests()
    }
  }
}
</script>


