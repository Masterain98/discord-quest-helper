<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { ChevronDown, LogOut } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const authStore = useAuthStore()
const emit = defineEmits<{ logout: [] }>()

const user = computed(() => authStore.user)
const open = ref(false)
const containerRef = ref<HTMLElement | null>(null)

const nitroBadge = computed(() => {
  const pt = user.value?.premium_type
  if (!pt || pt === 0) return null
  if (pt === 1) return { label: t('user.nitro_classic'), class: 'border-sky-400/60 bg-sky-500/10 text-sky-600 dark:text-sky-400' }
  if (pt === 2) return { label: t('user.nitro'), class: 'border-violet-400/60 bg-violet-500/10 text-violet-600 dark:text-violet-400' }
  if (pt === 3) return { label: t('user.nitro_basic'), class: 'border-indigo-400/60 bg-indigo-500/10 text-indigo-600 dark:text-indigo-400' }
  return null
})

const avatarUrl = computed(() => {
  if (!user.value?.avatar) return null
  return `https://cdn.discordapp.com/avatars/${user.value.id}/${user.value.avatar}.png?size=128`
})

function handleClickOutside(e: MouseEvent) {
  if (containerRef.value && !containerRef.value.contains(e.target as Node)) {
    open.value = false
  }
}

function handleLogout() {
  open.value = false
  emit('logout')
}

onMounted(() => document.addEventListener('mousedown', handleClickOutside))
onUnmounted(() => document.removeEventListener('mousedown', handleClickOutside))
</script>

<template>
  <div v-if="user" ref="containerRef" class="relative">
    <!-- Trigger button — pure HTML, no Radix wrapper -->
    <button
      class="h-10 px-2 rounded-lg inline-flex items-center gap-2 hover:bg-muted/60 transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring"
      @click="open = !open"
    >
      <Avatar class="w-8 h-8 shrink-0">
        <AvatarImage v-if="avatarUrl" :src="avatarUrl" :alt="user.username" />
        <AvatarFallback>{{ user.username[0].toUpperCase() }}</AvatarFallback>
      </Avatar>

      <span class="hidden md:inline-flex flex-col items-start min-w-0 leading-tight">
        <span class="text-sm font-medium max-w-[120px] truncate">
          {{ user.global_name || user.username }}
        </span>
        <span class="text-xs text-muted-foreground max-w-[120px] truncate">
          @{{ user.username }}
        </span>
      </span>

      <ChevronDown
        class="w-4 h-4 text-muted-foreground shrink-0 hidden md:block transition-transform"
        :class="open && 'rotate-180'"
      />
    </button>

    <!-- Dropdown content — positioned absolutely, no Radix portal -->
    <Transition
      enter-active-class="transition ease-out duration-150"
      enter-from-class="opacity-0 -translate-y-1 scale-95"
      enter-to-class="opacity-100 translate-y-0 scale-100"
      leave-active-class="transition ease-in duration-100"
      leave-from-class="opacity-100 translate-y-0 scale-100"
      leave-to-class="opacity-0 -translate-y-1 scale-95"
    >
      <div
        v-if="open"
        class="absolute right-0 top-full mt-2 z-50 w-56 rounded-lg border bg-popover text-popover-foreground shadow-md overflow-hidden"
      >
        <!-- User info header -->
        <div class="px-3 py-3">
          <div class="flex items-center gap-3">
            <Avatar class="w-10 h-10 shrink-0">
              <AvatarImage v-if="avatarUrl" :src="avatarUrl" :alt="user.username" />
              <AvatarFallback>{{ user.username[0].toUpperCase() }}</AvatarFallback>
            </Avatar>
            <div class="flex-1 min-w-0">
              <p class="text-sm font-medium truncate">{{ user.global_name || user.username }}</p>
              <p class="text-xs text-muted-foreground truncate">@{{ user.username }}</p>
              <span
                v-if="nitroBadge"
                :class="['inline-flex mt-1 text-[10px] font-semibold px-1.5 py-0.5 rounded-full border', nitroBadge.class]"
              >
                {{ nitroBadge.label }}
              </span>
            </div>
          </div>
        </div>

        <div class="h-px bg-border mx-2" />

        <!-- Logout -->
        <button
          class="w-full px-3 py-2 text-left text-sm text-destructive hover:bg-destructive/10 transition-colors inline-flex items-center gap-2"
          @click="handleLogout"
        >
          <LogOut class="w-4 h-4" />
          {{ t('general.logout') }}
        </button>
      </div>
    </Transition>
  </div>
</template>
