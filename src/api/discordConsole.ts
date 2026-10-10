import { Channel, invoke } from '@tauri-apps/api/core'

export interface RemoteObject {
  type: string
  subtype?: string
  className?: string
  value?: unknown
  unserializableValue?: string
  description?: string
  objectId?: string
  preview?: { description?: string; overflow?: boolean; properties?: Array<{ name: string; type: string; value?: string }> }
}
export interface ConsoleException {
  text: string
  exception?: RemoteObject
  lineNumber?: number
  columnNumber?: number
  url?: string
  stackTrace?: { callFrames: Array<{ functionName: string; url: string; lineNumber: number; columnNumber: number }> }
}
export interface ConsoleEvaluation { result: RemoteObject; exceptionDetails?: ConsoleException }
export interface ConsoleProperty {
  name: string
  value?: RemoteObject
  get?: RemoteObject
  set?: RemoteObject
  symbol?: RemoteObject
}
export interface ConsoleProperties { result: ConsoleProperty[]; internalProperties?: ConsoleProperty[]; exceptionDetails?: ConsoleException }
export interface ConsoleSession { sessionId: string; targetTitle: string; targetUrl: string; port: number }
export type ConsoleLevel = 'log' | 'info' | 'debug' | 'warn' | 'error'
export interface ConsoleEvent {
  sessionId: string
  sequence: number
  timestamp: number
  kind: 'console' | 'exception' | 'disconnected'
  level: ConsoleLevel
  args: RemoteObject[]
  exception?: ConsoleException | null
  message?: string | null
}
export const openDiscordConsole = (port: number, onEvent: (event: ConsoleEvent) => void) =>
  invoke<ConsoleSession>('open_discord_console', { port, onEvent: new Channel<ConsoleEvent>(onEvent) })
export const evaluateDiscordConsole = (sessionId: string, expression: string) =>
  invoke<ConsoleEvaluation>('evaluate_discord_console', { sessionId, expression })
export const getDiscordConsoleProperties = (sessionId: string, objectId: string) =>
  invoke<ConsoleProperties>('get_discord_console_properties', { sessionId, objectId })
export const releaseDiscordConsoleObjects = (sessionId: string, objectIds: string[]) =>
  invoke<void>('release_discord_console_objects', { sessionId, objectIds })
export const closeDiscordConsole = (sessionId: string) => invoke<void>('close_discord_console', { sessionId })
