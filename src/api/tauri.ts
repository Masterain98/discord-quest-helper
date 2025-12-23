import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export interface DiscordUser {
  id: string
  username: string
  discriminator: string
  avatar: string | null
  global_name: string | null
}

export interface Quest {
  id: string
  config: {
    messages: {
      quest_name: string
      game_title?: string
    }
    rewards_config?: {
      rewards: Array<{
        messages: {
          name: string
        }
        asset?: string
      }>
    }
    stream_duration_requirement_minutes?: number
    task_config?: {
      tasks?: Record<string, { target?: number }>
    }
    task_config_v2?: {
      tasks?: Record<string, { target?: number }>
    }
    application?: {
      id: string
      name: string
      link: string
      icon?: string
    }
    assets?: {
      hero?: string
    }
    expires_at?: string
  }
  user_status: {
    progress?: Record<string, { value?: number }>
    completed_at?: string | null
    claimed_at?: string | null
    enrolled_at?: string | null
  } | null
}

export interface DetectableGame {
  id: string
  name: string
  executables: Array<{
    name: string
    os: string
  }>
}

// Auth commands
export interface ExtractedAccount {
  token: string
  user: DiscordUser
}

export async function autoDetectToken(): Promise<ExtractedAccount[]> {
  return await invoke('auto_detect_token')
}

export async function setToken(token: string): Promise<DiscordUser> {
  return await invoke('set_token', { token })
}

// RPC commands
export function connectToDiscordRpc(activityJson: string, action: string = 'connect'): Promise<void> {
  return invoke('connect_to_discord_rpc', { activity_json: activityJson, action })
}

// User status commands
export async function getQuests(): Promise<Quest[]> {
  return await invoke('get_quests')
}

export async function startVideoQuest(
  questId: string,
  secondsNeeded: number,
  initialProgress: number,
  speedMultiplier: number,
  heartbeatInterval: number
): Promise<void> {
  return await invoke('start_video_quest', {
    questId,
    secondsNeeded,
    initialProgress,
    speedMultiplier,
    heartbeatInterval
  })
}

export async function startStreamQuest(
  questId: string,
  streamKey: string,
  secondsNeeded: number,
  initialProgress: number
): Promise<void> {
  return await invoke('start_stream_quest', {
    questId,
    streamKey,
    secondsNeeded,
    initialProgress
  })
}

export async function stopQuest(): Promise<void> {
  return await invoke('stop_quest')
}

export async function startGameHeartbeatQuest(
  questId: string,
  applicationId: string,
  secondsNeeded: number,
  initialProgress: number
): Promise<void> {
  return await invoke('start_game_heartbeat_quest', {
    questId,
    applicationId,
    secondsNeeded,
    initialProgress
  })
}

// Game simulator commands
export async function createSimulatedGame(
  path: string,
  executableName: string,
  appId: string
): Promise<void> {
  return await invoke('create_simulated_game', {
    path,
    executableName,
    appId
  })
}

export async function runSimulatedGame(
  name: string,
  path: string,
  executableName: string,
  appId: string
): Promise<void> {
  return await invoke('run_simulated_game', {
    name,
    path,
    executableName,
    appId
  })
}

export async function stopSimulatedGame(execName: string): Promise<void> {
  return await invoke('stop_simulated_game', { execName })
}

export async function fetchDetectableGames(): Promise<DetectableGame[]> {
  return await invoke('fetch_detectable_games')
}

export async function acceptQuest(questId: string): Promise<void> {
  return await invoke('accept_quest', { questId })
}

// Event listeners
export function onQuestProgress(callback: (progress: number) => void) {
  return listen<number>('quest-progress', (event) => {
    callback(event.payload)
  })
}

export function onQuestComplete(callback: () => void) {
  return listen('quest-complete', () => {
    callback()
  })
}

export function onQuestError(callback: (error: string) => void) {
  return listen<string>('quest-error', (event) => {
    callback(event.payload)
  })
}

export async function forceVideoProgress(questId: string, timestamp: number): Promise<void> {
  return await invoke('force_video_progress', { questId, timestamp })
}
