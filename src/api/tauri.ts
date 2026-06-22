import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export interface DiscordUser {
  id: string
  username: string
  discriminator: string
  avatar: string | null
  global_name: string | null
  /** Nitro subscription type: 0=None, 1=Nitro Classic, 2=Nitro, 3=Nitro Basic */
  premium_type?: number | null
}

export interface Quest {
  id: string
  traffic_metadata_raw?: string | null
  traffic_metadata_sealed?: string | null
  config: {
    id?: string
    messages: {
      quest_name: string
      game_title?: string
      task_title?: string
      task_description?: string
    }
    rewards_config?: {
      rewards: QuestReward[]
    }
    stream_duration_requirement_minutes?: number
    task_config?: {
      tasks?: Record<string, QuestTaskConfigEntry>
    }
    task_config_v2?: {
      tasks?: Record<string, QuestTaskConfigEntry>
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
    features?: string[]
    share_policy?: unknown
    cta_config?: {
      link?: string | null
      [key: string]: unknown
    }
    preview?: unknown
    targeted_content?: unknown
    traffic_metadata_raw?: string | null
    traffic_metadata_sealed?: string | null
  }
  user_status: QuestUserStatus | null
}

export interface QuestReward {
  type: number
  sku_id: string
  messages: {
    name: string
    name_with_article?: string
    redemption_instructions_by_platform?: Record<string, string>
  }
  asset?: string | null
  asset_video?: string | null
  approximate_count?: number | null
  redemption_link?: string | null
  expires_at?: string | null
  expires_at_premium?: string | null
  expiration_mode?: number | null
  orb_quantity?: number | null
  premium_orb_quantity?: number | null
  quantity?: number | null
}

export interface QuestTaskConfigEntry {
  type?: string
  target?: number
  applications?: Array<{ id: string }>
  external_ids?: string[]
  assets?: unknown
  messages?: Record<string, string>
  event_name?: string
}

export interface QuestTaskProgress {
  value?: number
  target?: number
  completed_at?: string | null
}

export interface QuestUserStatus {
  user_id?: string
  quest_id?: string
  enrolled_at?: string | null
  completed_at?: string | null
  claimed_at?: string | null
  claimed_tier?: number | null
  last_stream_heartbeat_at?: string | null
  stream_progress_seconds?: number | null
  dismissed_quest_content?: number | null
  progress?: Record<string, QuestTaskProgress>
  orb_quantity_claimed?: number | null
}

export interface ExcludedQuest {
  id: string
  replacement_id?: string | null
}

export interface CurrentUserQuestsResponse {
  quests: Quest[]
  excluded_quests: ExcludedQuest[]
  quest_enrollment_blocked_until?: string | null
}

export interface DetectableGame {
  id: string
  name: string
  executables: Array<{
    name: string
    os: string
  }>
  icon?: string
  type_name?: string
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

export async function getQuestsFull(): Promise<CurrentUserQuestsResponse> {
  return await invoke('get_quests_full')
}

export async function getVirtualCurrencyBalance(): Promise<number> {
  const response = await invoke<{ balance?: number }>('get_virtual_currency_balance')
  return response.balance ?? 0
}

export async function getQuestDecisionDebug(placement: number): Promise<unknown> {
  return await invoke('get_quest_decision_debug', { placement })
}

export async function getQuestDecisionsDebug(placement: number, num: number): Promise<unknown> {
  return await invoke('get_quest_decisions_debug', { placement, num })
}

export async function claimQuestReward(questId: string, platform?: string): Promise<unknown> {
  return await invoke('claim_quest_reward', { questId, platform })
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

// [LEGACY] Direct heartbeat mode — kept for backward compatibility
// Use startCdpQuest() instead for new code
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

// Debug info types
export interface SuperProperties {
  os: string
  browser: string
  release_channel: string
  client_version?: string
  os_version: string
  os_arch?: string
  app_arch?: string
  system_locale: string
  has_client_mods: boolean
  browser_user_agent: string
  browser_version: string
  os_sdk_version?: string
  client_build_number: number
  native_build_number?: number
  client_event_source: string | null
  launch_signature?: string
  client_launch_id?: string
  client_heartbeat_session_id?: string
  client_app_state?: string
}

export interface DebugInfo {
  x_super_properties_base64?: string
  super_properties?: SuperProperties
  client_launch_id?: string
  client_heartbeat_session_id?: string
  launch_signature?: string
  source?: string  // "Auto-Generated" or "Discord Client (Extracted)"
  client_identity?: ClientIdentitySnapshot
  header_profile?: HeaderProfilePreview
}

export interface ClientIdentitySnapshot {
  user_agent: string
  client_version?: string
  browser_version: string
  client_build_number?: number
  native_build_number?: number
  source: string
}

export interface HeaderProfilePreview {
  timezone: string
  timezone_source: string
  locale: string
  locale_source: string
  accept_language: string
  accept_language_source: string
  installation_id_present: boolean
  installation_id_source: string
}

export async function getDebugInfo(): Promise<DebugInfo> {
  return await invoke('get_debug_info')
}

// Runner information
export interface RunnerInfo {
  embedded: boolean
  commit_hash: string
  build_time: string
  size_bytes: number
}

export async function getRunnerInfo(): Promise<RunnerInfo> {
  return await invoke('get_runner_info')
}

// CDP (Chrome DevTools Protocol) types and commands
export interface CdpStatus {
  available: boolean
  connected: boolean
  target_title: string | null
  error: string | null
}

export interface CdpSuperProperties {
  base64: string
  decoded: SuperProperties
}

export async function checkCdpStatus(port?: number): Promise<CdpStatus> {
  return await invoke('check_cdp_status', { port })
}

export async function fetchSuperPropertiesCdp(port?: number): Promise<CdpSuperProperties> {
  return await invoke('fetch_super_properties_cdp', { port })
}

export type DiscordChannelArg = 'auto' | 'stable' | 'ptb' | 'canary'
export type DiscordChannelResult = 'stable' | 'ptb' | 'canary'

export interface DiscordCdpLaunchResult {
  launched_path: string
  channel: DiscordChannelResult
  port: number
  cdp_connected: boolean
}

export async function isDiscordRunning(channel?: DiscordChannelArg): Promise<boolean> {
  return await invoke('is_discord_running', { channel })
}

export async function launchDiscordCdp(port?: number, channel?: DiscordChannelArg): Promise<DiscordCdpLaunchResult> {
  return await invoke('launch_discord_cdp', { port, channel })
}

export async function restartDiscordCdp(port?: number, channel?: DiscordChannelArg): Promise<DiscordCdpLaunchResult> {
  return await invoke('restart_discord_cdp', { port, channel })
}

export async function createDiscordCdpLauncherShortcut(port?: number, channel?: DiscordChannelArg): Promise<string> {
  return await invoke('create_discord_cdp_launcher_shortcut', { port, channel })
}

export async function createDiscordDebugShortcut(port?: number): Promise<string> {
  return await invoke('create_discord_debug_shortcut', { port })
}

// SuperProperties Mode types and commands
export type SuperPropertiesMode = 'cdp' | 'remote_js' | 'default'

export interface SuperPropertiesModeInfo {
  mode: SuperPropertiesMode
  mode_display: string
  build_number: number | null
}

export interface AutoFetchResult {
  success: boolean
  mode: SuperPropertiesMode
  build_number: number | null
}

export async function getSuperPropertiesMode(): Promise<SuperPropertiesModeInfo> {
  return await invoke('get_super_properties_mode')
}

export async function autoFetchSuperProperties(cdpPort?: number): Promise<AutoFetchResult> {
  return await invoke('auto_fetch_super_properties', { cdpPort })
}

export async function retrySuperProperties(cdpPort?: number): Promise<AutoFetchResult> {
  return await invoke('retry_super_properties', { cdpPort })
}

// CDP captured headers (full network capture)
export interface CapturedRequest {
  url: string
  method: string
  headers: Record<string, string>
}

export interface CdpCapturedHeaders {
  total_requests: number
  requests: CapturedRequest[]
  header_key_counts: Record<string, number>
  header_kv_counts: Record<string, number>
  capture_duration_secs: number
}

export async function captureDiscordHeadersCdp(port?: number, durationSecs?: number): Promise<CdpCapturedHeaders> {
  return await invoke('capture_discord_headers_cdp', { port, durationSecs })
}

// CDP Quest Completion

export async function startCdpQuest(
  questId: string,
  questType: 'play' | 'stream' | 'video' | 'activity',
  applicationId: string,
  applicationName: string,
  secondsNeeded: number,
  initialProgress: number,
  cdpPort: number,
  checkpointTimes?: number[]
): Promise<void> {
  return await invoke('start_cdp_quest', {
    questId,
    questType,
    applicationId,
    applicationName,
    secondsNeeded,
    initialProgress,
    cdpPort,
    checkpointTimes: checkpointTimes || []
  })
}

export async function navigateDiscordSpa(targetPath: string, cdpPort: number): Promise<void> {
  return await invoke('navigate_discord_spa', { targetPath, cdpPort })
}
