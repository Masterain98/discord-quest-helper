export type SettingsTone =
  | 'primary'
  | 'success'
  | 'warning'
  | 'danger'
  | 'info'
  | 'neutral'
  | 'violet'

export const settingToneClass = {
  primary: {
    card: 'border-primary/25 bg-primary/5',
    panel: 'border-primary/30 bg-primary/10 text-primary',
    icon: 'bg-primary/15 text-primary',
    badge: 'border-primary/40 bg-primary/10 text-primary',
    buttonSoft: 'border-primary/40 bg-primary/10 text-primary hover:bg-primary/15 hover:text-primary',
  },
  success: {
    card: 'border-emerald-500/25 bg-emerald-500/5',
    panel: 'border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300',
    icon: 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300',
    badge: 'border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300',
    buttonSoft: 'border-emerald-500/40 bg-emerald-500/10 text-emerald-700 hover:bg-emerald-500/15 hover:text-emerald-700 dark:text-emerald-300 dark:hover:text-emerald-300',
  },
  warning: {
    card: 'border-amber-500/25 bg-amber-500/5',
    panel: 'border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300',
    icon: 'bg-amber-500/15 text-amber-700 dark:text-amber-300',
    badge: 'border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-300',
    buttonSoft: 'border-amber-500/40 bg-amber-500/10 text-amber-700 hover:bg-amber-500/15 hover:text-amber-700 dark:text-amber-300 dark:hover:text-amber-300',
  },
  danger: {
    card: 'border-destructive/25 bg-destructive/5',
    panel: 'border-destructive/30 bg-destructive/10 text-destructive',
    icon: 'bg-destructive/15 text-destructive',
    badge: 'border-destructive/40 bg-destructive/10 text-destructive',
    buttonSoft: 'border-destructive/40 bg-destructive/10 text-destructive hover:bg-destructive/15 hover:text-destructive',
  },
  info: {
    card: 'border-sky-500/25 bg-sky-500/5',
    panel: 'border-sky-500/30 bg-sky-500/10 text-sky-700 dark:text-sky-300',
    icon: 'bg-sky-500/15 text-sky-700 dark:text-sky-300',
    badge: 'border-sky-500/40 bg-sky-500/10 text-sky-700 dark:text-sky-300',
    buttonSoft: 'border-sky-500/40 bg-sky-500/10 text-sky-700 hover:bg-sky-500/15 hover:text-sky-700 dark:text-sky-300 dark:hover:text-sky-300',
  },
  violet: {
    card: 'border-violet-500/25 bg-violet-500/5',
    panel: 'border-violet-500/30 bg-violet-500/10 text-violet-700 dark:text-violet-300',
    icon: 'bg-violet-500/15 text-violet-700 dark:text-violet-300',
    badge: 'border-violet-500/40 bg-violet-500/10 text-violet-700 dark:text-violet-300',
    buttonSoft: 'border-violet-500/40 bg-violet-500/10 text-violet-700 hover:bg-violet-500/15 hover:text-violet-700 dark:text-violet-300 dark:hover:text-violet-300',
  },
  neutral: {
    card: 'border-border bg-card',
    panel: 'border-border bg-muted/40 text-foreground',
    icon: 'bg-muted text-muted-foreground',
    badge: 'border-border bg-muted/50 text-muted-foreground',
    buttonSoft: 'border-border bg-background hover:bg-muted hover:text-foreground',
  },
} as const
