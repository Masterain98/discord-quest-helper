import { createI18n } from 'vue-i18n'
import en from './locales/en'
import zh from './locales/zh'
import zhTW from './locales/zh-TW'
import ru from './locales/ru'
import ja from './locales/ja'
import ko from './locales/ko'
import es from './locales/es'
import th from './locales/th'
import ptBR from './locales/pt-BR'
import tr from './locales/tr'
import vi from './locales/vi'
import de from './locales/de'
import fr from './locales/fr'
import ptPT from './locales/pt-PT'
import id from './locales/id'
import pl from './locales/pl'

// Detect default locale based on browser settings
function getDefaultLocale(): string {
    const normalizeLocale = (raw: string): string => {
        const v = raw.toLowerCase()
        if (v.startsWith('zh-tw') || v.startsWith('zh-hant')) return 'zh-TW'
        if (v.startsWith('pt-br')) return 'pt-BR'
        if (v === 'pt' || v.startsWith('pt-pt')) return 'pt-PT'
        if (v.startsWith('zh')) return 'zh'
        if (v.startsWith('th')) return 'th'
        if (v.startsWith('tr')) return 'tr'
        if (v.startsWith('vi')) return 'vi'
        if (v.startsWith('de')) return 'de'
        if (v.startsWith('fr')) return 'fr'
        if (v.startsWith('id')) return 'id'
        if (v.startsWith('pl')) return 'pl'
        if (v.startsWith('ja')) return 'ja'
        if (v.startsWith('ko')) return 'ko'
        if (v.startsWith('ru')) return 'ru'
        if (v.startsWith('es')) return 'es'
        return 'en'
    }

    const saved = localStorage.getItem('locale') ?? localStorage.getItem('language')
    if (saved) return normalizeLocale(saved)

    return normalizeLocale(navigator.language)
}

const i18n = createI18n({
    legacy: false, // Use Composition API mode
    locale: getDefaultLocale(),
    fallbackLocale: 'en',
    messages: {
        en,
        zh,
        'zh-TW': zhTW,
        ru,
        ja,
        ko,
        es,
        th,
        'pt-BR': ptBR,
        tr,
        vi,
        de,
        fr,
        'pt-PT': ptPT,
        id,
        pl
    }
})

export default i18n

