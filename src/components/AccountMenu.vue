<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useUserBadgesStore } from '@/stores/userBadges'
import { ACCOUNT_BADGES, badgeCount, badgeIcon } from '@/utils/userBadges'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { DropdownMenuItem } from '@/components/ui/dropdown-menu'
import { ChevronDown, Gamepad2, LogOut, Swords, Trophy } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import IconMenu from './IconMenu.vue'

withDefaults(defineProps<{ compact?: boolean; suspended?: boolean }>(), { compact: false, suspended: false })
const { t, locale } = useI18n()
const authStore = useAuthStore()
const badgesStore = useUserBadgesStore()
const emit = defineEmits<{ logout: []; farm: [] }>()
const user = computed(() => authStore.user)
const label = computed(() => t('sidebar.account_menu', { name: user.value?.global_name || user.value?.username }))
const open = ref(false)
const failedIcons = ref(new Set<number>())
const badgeRows = computed(() => ACCOUNT_BADGES.flatMap(definition => {
  const badge = badgesStore.badges.find(b => b.badge_id === definition.id && b.owned === true)
  if (!badge) return []
  const count = badgeCount(badge, definition.unit)
  return [{
    ...definition,
    icon: badgeIcon(badge),
    fallbackIcon: { games: Gamepad2, quests: Trophy, hours: Swords }[definition.kind],
    text: count === null && typeof badge?.info_label === 'string' && badge.info_label
      ? badge.info_label
      : t(`sidebar.badges_${definition.kind}`, { count: count?.toLocaleString(locale.value) ?? '—' }),
  }]
}))
watch(() => badgesStore.badges, () => { failedIcons.value = new Set() })
watch(open, visible => { if (visible) void badgesStore.refresh() })
const avatarUrl = computed(() => user.value?.avatar
  ? `https://cdn.discordapp.com/avatars/${user.value.id}/${user.value.avatar}.png?size=128` : undefined)
function handleLogout() { open.value = false; emit('logout') }
function handleFarm() { open.value = false; emit('farm') }
</script>

<template>
  <IconMenu v-if="user" v-model:open="open" :label="label" :side="compact ? 'right' : 'bottom'" :suspended="suspended" content-class="w-64">
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
      <ul v-if="badgeRows.length" class="account-badges mx-2 mb-2 space-y-2 border-t border-border pt-3 pb-1" :aria-label="t('sidebar.badges')" :aria-busy="badgesStore.loading">
        <li v-for="badge in badgeRows" :key="badge.id" class="flex items-center gap-3 text-xs leading-5">
          <img v-if="badge.icon && !failedIcons.has(badge.id)" :src="badge.icon" alt="" aria-hidden="true" class="h-5 w-5 shrink-0 object-contain" @error="failedIcons.add(badge.id)" />
          <component :is="badge.fallbackIcon" v-else class="h-5 w-5 shrink-0 text-muted-foreground" :stroke-width="1.5" aria-hidden="true" />
          <span class="min-w-0 flex-1 tabular-nums" :class="!badgesStore.badges.length && 'text-muted-foreground'">{{ badge.text }}</span>
          <DropdownMenuItem v-if="badge.kind !== 'quests'" as="button" type="button" class="shrink-0 cursor-pointer px-1.5 py-0.5 text-xs text-primary focus:bg-primary/10 focus:text-primary" @select="handleFarm">
            {{ t('sidebar.badges_farm') }}
          </DropdownMenuItem>
        </li>
      </ul>
      <p v-if="badgesStore.failed && !badgesStore.badges.length" class="px-2 pb-2 text-xs text-muted-foreground" role="status">{{ t('sidebar.badges_unavailable') }}</p>
      <div class="mx-2 mb-1 h-px bg-border" />
      <DropdownMenuItem class="gap-2 text-destructive focus:bg-destructive/10 focus:text-destructive" @select="handleLogout">
        <LogOut class="h-4 w-4" />{{ t('general.logout') }}
      </DropdownMenuItem>
  </IconMenu>
</template>
