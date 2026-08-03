import { spawnSync } from 'node:child_process'

// A Snap-packaged IDE exports its private GTK/GIO runtime into integrated
// terminals. Those paths can leak into a host Tauri process and make WebKit
// load Snap's incompatible glibc libraries. Keep the normal environment for
// every other launch context and sanitize only when a Snap runtime is detected.
const env = { ...process.env }

const snapRuntimeKeys = [
  'GDK_PIXBUF_MODULEDIR',
  'GDK_PIXBUF_MODULE_FILE',
  'GIO_LAUNCHED_DESKTOP_FILE',
  'GIO_MODULE_DIR',
  'GSETTINGS_SCHEMA_DIR',
  'GTK_EXE_PREFIX',
  'GTK_IM_MODULE_FILE',
  'GTK_MODULES',
  'GTK_PATH',
  'LOCPATH',
  'VSCODE_NLS_CONFIG',
]

const hasSnapRuntime = Object.entries(env).some(([key, value]) => {
  if (key === 'SNAP' || key.startsWith('SNAP_')) return true
  if (!snapRuntimeKeys.includes(key) && !key.startsWith('XDG_DATA_')) return false
  return typeof value === 'string' && (value.includes('/snap/') || value.includes('/snapd/'))
})

if (process.platform === 'linux' && hasSnapRuntime) {
  console.log('Detected Snap-injected GTK runtime; using host libraries for Tauri dev.')

  for (const key of snapRuntimeKeys) delete env[key]
  for (const key of Object.keys(env)) {
    if (key === 'SNAP' || key.startsWith('SNAP_')) delete env[key]
  }

  // VS Code preserves the host value for this variable specifically so child
  // processes can restore it after leaving the Snap environment.
  if (env.XDG_DATA_DIRS_VSCODE_SNAP_ORIG) {
    env.XDG_DATA_DIRS = env.XDG_DATA_DIRS_VSCODE_SNAP_ORIG
  } else if (env.XDG_DATA_DIRS) {
    env.XDG_DATA_DIRS = env.XDG_DATA_DIRS
      .split(':')
      .filter((entry) => !entry.includes('/snap/') && !entry.includes('/snapd/'))
      .join(':')
  }
  delete env.XDG_DATA_DIRS_VSCODE_SNAP_ORIG
  if (env.XDG_DATA_HOME?.includes('/snap/')) delete env.XDG_DATA_HOME
}

const result = spawnSync(
  'tauri',
  ['dev', '--', '--bin', 'discord-quest-helper'],
  { stdio: 'inherit', env },
)

if (result.error) {
  console.error(`Failed to start Tauri: ${result.error.message}`)
  process.exit(1)
}

process.exit(result.status ?? 1)
