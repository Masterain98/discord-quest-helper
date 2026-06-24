import { createI18n } from 'vue-i18n'
import en from './locales/en.json'
import zh from './locales/zh.json'
import zhTW from './locales/zh-TW.json'
import ru from './locales/ru.json'
import ja from './locales/ja.json'
import ko from './locales/ko.json'
import es from './locales/es.json'
import th from './locales/th.json'
import ptBR from './locales/pt-BR.json'
import tr from './locales/tr.json'
import vi from './locales/vi.json'
import de from './locales/de.json'
import fr from './locales/fr.json'
import ptPT from './locales/pt-PT.json'
import id from './locales/id.json'
import pl from './locales/pl.json'

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

