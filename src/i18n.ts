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
    const saved = localStorage.getItem('locale') ?? localStorage.getItem('language')
    if (saved) return saved

    const browserLang = navigator.language.toLowerCase()
    if (browserLang.startsWith('zh-tw') || browserLang.startsWith('zh-hant')) return 'zh-TW'
    if (browserLang.startsWith('pt-br')) return 'pt-BR'
    if (browserLang.startsWith('zh')) return 'zh'
    if (browserLang.startsWith('th')) return 'th'
    if (browserLang.startsWith('tr')) return 'tr'
    if (browserLang.startsWith('vi')) return 'vi'
    if (browserLang.startsWith('de')) return 'de'
    if (browserLang.startsWith('fr')) return 'fr'
    if (browserLang.startsWith('pt')) return 'pt-PT'
    if (browserLang.startsWith('id')) return 'id'
    if (browserLang.startsWith('pl')) return 'pl'
    if (browserLang.startsWith('ja')) return 'ja'
    if (browserLang.startsWith('ko')) return 'ko'
    if (browserLang.startsWith('ru')) return 'ru'
    if (browserLang.startsWith('es')) return 'es'
    return 'en'
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

