import type { CdpDiagnosticSnapshot, CdpRuntime } from '@/api/tauri'
import type { TimestampedCommandError } from '@/composables/cdpDiagnostics'

function basename(path: string | null): string | null {
  const name = path?.split(/[\\/]/).filter(Boolean).pop()
  return name ? redactText(name) : null
}

function redactText(value: string): string {
  return value
    .replace(/[A-Z]:\\Users\\[^\\]+\\/gi, '%USERPROFILE%\\')
    .replace(/\/(?:home|Users)\/[^/]+\//g, '~/')
    .replace(/[\w.+-]+@[\w.-]+\.[A-Za-z]{2,}/g, '[redacted-email]')
    .replace(/\b\d{17,20}\b/g, '[redacted-id]')
    .replace(/\b(?:Bearer|Bot)\s+\S+/gi, '[redacted-authorization]')
}

function safeErrorParams(params: Record<string, unknown> | null): Record<string, unknown> | null {
  if (!params) return null
  const numbers = new Set(['port', 'timeoutMs', 'targetCount', 'discordTargetCount'])
  const safe: Record<string, number | boolean | string> = {}
  for (const [key, value] of Object.entries(params)) {
    if (numbers.has(key) && typeof value === 'number' && Number.isFinite(value)) {
      safe[key] = value
    } else if (key === 'mainRendererFound' && typeof value === 'boolean') {
      safe[key] = value
    } else if (key === 'lastStatus' && typeof value === 'string' &&
      ['unreachable', 'portOccupied', 'cdpWithoutDiscordTarget', 'discordReady'].includes(value)) {
      safe[key] = value
    }
  }
  return safe
}

export function sanitizeCdpLaunchError(lastError: TimestampedCommandError | null) {
  return lastError ? {
    code: lastError.code ? redactText(lastError.code) : null,
    message: redactText(lastError.message),
    params: safeErrorParams(lastError.params),
    timestamp: lastError.timestamp,
  } : null
}

function sanitizeRuntime(runtime: CdpRuntime) {
  return {
    runtimeStatus: runtime.runtimeStatus,
    webSocketReachable: runtime.webSocketReachable,
    appRootPresent: runtime.appRootPresent,
    moduleLoaderPresent: runtime.moduleLoaderPresent,
    nativeBridgePresent: runtime.nativeBridgePresent,
    failureStage: runtime.failureStage ? redactText(runtime.failureStage) : null,
    reasonCode: runtime.reasonCode ? redactText(runtime.reasonCode) : null,
  }
}

export function sanitizeCdpDiagnosticExport(
  snapshot: CdpDiagnosticSnapshot,
  lastError: TimestampedCommandError | null,
) {
  const classifications = snapshot.targets.reduce<Record<string, number>>((counts, target) => {
    counts[target.classification] = (counts[target.classification] ?? 0) + 1
    return counts
  }, {})
  return {
    timestamp: snapshot.timestamp,
    port: snapshot.port,
    endpointStatus: snapshot.endpointStatus,
    endpointOwner: redactText(snapshot.endpointOwner),
    ownerProviderId: snapshot.ownerProviderId ? redactText(snapshot.ownerProviderId) : null,
    selectedClient: snapshot.selectedClient ? redactText(snapshot.selectedClient) : null,
    selectedProviderId: snapshot.selectedProviderId ? redactText(snapshot.selectedProviderId) : null,
    selectedVariantId: snapshot.selectedVariantId ? redactText(snapshot.selectedVariantId) : null,
    selectedExecutable: basename(snapshot.selectedExecutablePath),
    selectedRunning: snapshot.selectedRunning,
    portListening: snapshot.portListening,
    cdpHttpReachable: snapshot.cdpHttpReachable,
    cdpHttpStatus: snapshot.cdpHttpStatus,
    cdpResponseParseable: snapshot.cdpResponseParseable,
    launchRequestedPort: snapshot.port,
    processes: snapshot.processes.map(process => ({
      processName: redactText(process.processName),
      providerId: process.providerId ? redactText(process.providerId) : null,
      isSelectedInstallation: process.isSelectedInstallation,
      executable: basename(process.executablePath),
      hasRemoteDebuggingPortArg: process.hasRemoteDebuggingPortArg,
      remoteDebuggingPort: process.remoteDebuggingPort,
    })),
    targets: {
      total: snapshot.cdpTargetCount,
      discord: snapshot.discordTargetCount,
      mainRendererFound: snapshot.mainRendererFound,
      ...(snapshot.runtime ? { runtime: sanitizeRuntime(snapshot.runtime) } : {}),
      classifications,
    },
    lastLaunchError: sanitizeCdpLaunchError(lastError),
  }
}
