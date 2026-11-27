import { invoke } from '@tauri-apps/api/core'

export type ProxyMode = 'system' | 'direct' | 'manual'
export type ProxySource = 'direct' | 'environment' | 'system' | 'manual' | 'unavailable'
export interface NetworkSettings { mode: ProxyMode; address: string | null }
export type ProxyCredentialEdit =
  | { action: 'keep' | 'remove' }
  | { action: 'replace'; username: string; password: string }
export interface NetworkStatus {
  settings: NetworkSettings
  discord: { source: ProxySource; endpoint: string | null } | null
  pac_configured: boolean
  storage_invalid: boolean
  has_credentials: boolean
  credential_cleanup_pending: boolean
}
export interface NetworkConnectionTest { ip: string; elapsed_ms: number }

export const getNetworkSettings = () => invoke<NetworkStatus>('get_network_settings')
export const saveNetworkSettings = (settings: NetworkSettings, credentials: ProxyCredentialEdit) => invoke<NetworkStatus>('save_network_settings', { settings, credentials })
export const testNetworkConnection = (settings: NetworkSettings, credentials: ProxyCredentialEdit) => invoke<NetworkConnectionTest>('test_network_connection', { settings, credentials })
