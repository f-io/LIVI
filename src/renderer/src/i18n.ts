import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'

import en from './locales/en.json'
import de from './locales/de.json'
import ua from './locales/ua.json'

i18n.use(initReactI18next).init({
  debug: false,
  resources: {
    en: { translation: en },
    de: { translation: de },
    ua: { translation: ua }, // backward compatibility
    uk: { translation: ua }, // official ISO code (generic)
    'uk-UA': { translation: ua } // official ISO code (region)
  },
  lng: 'en',
  fallbackLng: 'en',
  supportedLngs: ['en', 'de', 'ua', 'uk', 'uk-UA'],
  nonExplicitSupportedLngs: true,
  interpolation: { escapeValue: false }
})

export default i18n
