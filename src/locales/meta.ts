export const supportedLocales = [
  { code: 'en', label: 'English' },
  { code: 'zh', label: '简体中文' },
  { code: 'zh-TW', label: '繁體中文' },
  { code: 'ja', label: '日本語' },
  { code: 'ko', label: '한국어' },
  { code: 'ru', label: 'Русский' },
  { code: 'es', label: 'Español' },
  { code: 'th', label: 'ไทย' },
  { code: 'pt-BR', label: 'Português (Brasil)' },
  { code: 'tr', label: 'Türkçe' },
  { code: 'vi', label: 'Tiếng Việt' },
  { code: 'de', label: 'Deutsch' },
  { code: 'fr', label: 'Français' },
  { code: 'pt-PT', label: 'Português (Portugal)' },
  { code: 'id', label: 'Bahasa Indonesia' },
  { code: 'pl', label: 'Polski' },
] as const

export type SupportedLocale = (typeof supportedLocales)[number]['code']
