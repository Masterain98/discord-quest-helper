import type {
  AuthProgress,
  CdpLaunchTarget,
  CdpStatus,
  DesktopClientArg,
  DesktopClientInventory,
  DiscordChannelArg,
} from '@/api/tauri'

export type { CdpLaunchTarget }

export type LoginMethod = 'local' | 'cdp' | 'manual'
export type LoginProgressState = 'running' | 'waiting' | 'success' | 'error' | 'neutral'
export type CdpAvailability = 'checking' | 'ready' | 'starting' | 'offline' | 'error'

export interface LoginProgressPresentation {
  key: string
  params?: Record<string, number>
  state: LoginProgressState
}

export function presentAuthProgress(progress: AuthProgress): LoginProgressPresentation {
  switch (progress.phase) {
    case 'extracting_tokens':
      return { key: 'auth.progress.extracting_tokens', state: 'running' }
    case 'validating_tokens':
      return {
        key: 'auth.progress.validating_tokens',
        params: { current: progress.current ?? 0, total: progress.total ?? 0 },
        state: 'running',
      }
    case 'accounts_found':
      return {
        key: 'auth.progress.accounts_found',
        params: { count: progress.valid_accounts ?? 0 },
        state: (progress.valid_accounts ?? 0) > 0 ? 'success' : 'error',
      }
    case 'validating_token':
      return { key: 'auth.progress.validating_token', state: 'running' }
    case 'capturing_cdp_session':
      return { key: 'auth.progress.capturing_cdp_session', state: 'running' }
    case 'validating_cdp_session':
      return { key: 'auth.progress.validating_cdp_session', state: 'running' }
    case 'preparing_session':
      return { key: 'auth.progress.preparing_session', state: 'running' }
    case 'syncing_client_info':
      return { key: 'auth.progress.syncing_client_info', state: 'running' }
    case 'complete':
      return { key: 'auth.progress.complete', state: 'success' }
  }
}

export function classifyCdpAvailability(
  checking: boolean,
  status: CdpStatus | null,
  probeFailed: boolean,
): CdpAvailability {
  if (checking && !status) return 'checking'
  if (probeFailed) return 'error'
  if (status?.connected) return 'ready'
  if (status?.available) return 'starting'
  return 'offline'
}

export function shouldPollCdp(options: {
  busy: boolean
  authenticated: boolean
  visible: boolean
}): boolean {
  return !options.busy && !options.authenticated && options.visible
}

export function canBeginLogin(activeMethod: LoginMethod | null, storeLoading: boolean): boolean {
  return activeMethod === null && !storeLoading
}

export function usesVesktopForCdpLogin(inventory: DesktopClientInventory | null): boolean {
  if (!inventory) return false
  if (inventory.cdpOwner === 'vesktop') return true
  return inventory.vesktopInstalled && !inventory.officialInstalled
}

export function installedCdpLaunchTargets(
  inventory: DesktopClientInventory | null,
): CdpLaunchTarget[] {
  if (!inventory) return []
  const targets: CdpLaunchTarget[] = []
  if (inventory.stableInstalled) targets.push('stable')
  if (inventory.ptbInstalled) targets.push('ptb')
  if (inventory.canaryInstalled) targets.push('canary')
  if (inventory.vesktopInstalled) targets.push('vesktop')
  return targets
}

export function shouldAskCdpLaunchTarget(
  cdpAvailable: boolean,
  targets: CdpLaunchTarget[],
): boolean {
  return !cdpAvailable && targets.length > 1
}

export function isCdpLaunchTargetRunning(
  inventory: DesktopClientInventory | null,
  target: CdpLaunchTarget | null,
): boolean {
  if (!inventory) return false
  if (target === 'vesktop') return inventory.vesktopRunning
  if (target === 'stable') return inventory.stableRunning
  if (target === 'ptb') return inventory.ptbRunning
  if (target === 'canary') return inventory.canaryRunning
  return inventory.officialRunning || inventory.vesktopRunning
}

export function launchArgsForCdpTarget(target: CdpLaunchTarget | null): {
  channel: DiscordChannelArg
  client: DesktopClientArg
} {
  if (target === 'vesktop') return { channel: 'auto', client: 'vesktop' }
  if (target === 'stable' || target === 'ptb' || target === 'canary') {
    return { channel: target, client: 'official' }
  }
  return { channel: 'auto', client: 'auto' }
}

export function startCdpPolling(callback: () => void, intervalMs = 5_000): () => void {
  const timer = setInterval(callback, intervalMs)
  return () => clearInterval(timer)
}
