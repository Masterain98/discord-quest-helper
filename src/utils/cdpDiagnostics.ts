import type { CdpDiagnosticSnapshot } from '@/api/tauri'
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
  const allowed = new Set([
    'port', 'timeoutMs', 'lastStatus', 'targetCount', 'discordTargetCount',
    'mainRendererFound', 'providerId', 'variantId', 'owner', 'client',
  ])
  return Object.fromEntries(Object.entries(params).filter(([key]) => allowed.has(key)))
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
    endpointOwner: snapshot.endpointOwner,
    ownerProviderId: snapshot.ownerProviderId,
    selectedClient: snapshot.selectedClient,
    selectedProviderId: snapshot.selectedProviderId,
    selectedVariantId: snapshot.selectedVariantId,
    selectedExecutable: basename(snapshot.selectedExecutablePath),
    selectedRunning: snapshot.selectedRunning,
    portListening: snapshot.portListening,
    cdpHttpReachable: snapshot.cdpHttpReachable,
    cdpHttpStatus: snapshot.cdpHttpStatus,
    cdpResponseParseable: snapshot.cdpResponseParseable,
    launchRequestedPort: snapshot.port,
    processes: snapshot.processes.map(process => ({
      processName: redactText(process.processName),
      providerId: process.providerId,
      isSelectedInstallation: process.isSelectedInstallation,
      executable: basename(process.executablePath),
      hasRemoteDebuggingPortArg: process.hasRemoteDebuggingPortArg,
      remoteDebuggingPort: process.remoteDebuggingPort,
    })),
    targets: {
      total: snapshot.cdpTargetCount,
      discord: snapshot.discordTargetCount,
      mainRendererFound: snapshot.mainRendererFound,
      classifications,
    },
    lastLaunchError: lastError ? {
      code: lastError.code,
      message: redactText(lastError.message),
      params: safeErrorParams(lastError.params),
      timestamp: lastError.timestamp,
    } : null,
  }
}
