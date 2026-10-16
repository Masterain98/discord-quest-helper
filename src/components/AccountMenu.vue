<script setup lang="ts">
import { ref, computed } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { DropdownMenuItem } from '@/components/ui/dropdown-menu'
import { ChevronDown, LogOut } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import IconMenu from './IconMenu.vue'

withDefaults(defineProps<{ compact?: boolean; suspended?: boolean }>(), { compact: false, suspended: false })
const { t } = useI18n()
const authStore = useAuthStore()
const emit = defineEmits<{ logout: [] }>()
const user = computed(() => authStore.user)
const label = computed(() => t('sidebar.account_menu', { name: user.value?.global_name || user.value?.username }))
const open = ref(false)
const avatarUrl = computed(() => user.value?.avatar
  ? `https://cdn.discordapp.com/avatars/${user.value.id}/${user.value.avatar}.png?size=128` : undefined)
function handleLogout() { open.value = false; emit('logout') }
</script>

<template>
  <IconMenu v-if="user" v-model:open="open" :label="label" :side="compact ? 'right' : 'bottom'" :suspended="suspended" content-class="w-56">
    <template #trigger>
        <button type="button" :aria-label="label" :class="compact ? 'app-rail-button' : 'inline-flex h-10 items-center gap-2 rounded-lg px-2 hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring'">
          <Avatar class="h-8 w-8 shrink-0">
            <AvatarImage v-if="avatarUrl" :src="avatarUrl" :alt="user.username" />
            <AvatarFallback>{{ user.username[0]?.toUpperCase() }}</AvatarFallback>
          </Avatar>
          <template v-if="!compact">
            <span class="hidden min-w-0 flex-col items-start leading-tight md:inline-flex">
              <span class="max-w-[120px] truncate text-sm font-medium">{{ user.global_name || user.username }}</span>
              <span class="max-w-[120px] truncate text-xs text-muted-foreground">@{{ user.username }}</span>
            </span>
            <ChevronDown class="hidden h-4 w-4 text-muted-foreground md:block" :class="open && 'rotate-180'" />
          </template>
        </button>
    </template>
      <div class="flex items-center gap-3 px-2 py-3">
        <Avatar class="h-10 w-10 shrink-0">
          <AvatarImage v-if="avatarUrl" :src="avatarUrl" :alt="user.username" />
          <AvatarFallback>{{ user.username[0]?.toUpperCase() }}</AvatarFallback>
        </Avatar>
        <div class="min-w-0">
          <p class="truncate text-sm font-medium">{{ user.global_name || user.username }}</p>
          <p class="truncate text-xs text-muted-foreground">@{{ user.username }}</p>
        </div>
      </div>
      <div class="mx-2 mb-1 h-px bg-border" />
      <DropdownMenuItem class="gap-2 text-destructive focus:bg-destructive/10 focus:text-destructive" @select="handleLogout">
        <LogOut class="h-4 w-4" />{{ t('general.logout') }}
      </DropdownMenuItem>
  </IconMenu>
</template>
