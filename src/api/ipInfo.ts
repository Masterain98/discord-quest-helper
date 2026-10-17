import { invoke } from '@tauri-apps/api/core'

export interface IpInfo {
  country: string
  countryCode: string
}

/** Uses the native HTTP client so this lookup is not blocked by WebView CORS. */
export function fetchIpInfo(): Promise<IpInfo> {
  return invoke<IpInfo>('get_ip_info')
}
