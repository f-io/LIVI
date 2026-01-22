import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'

import en from './locales/en.json'
import de from './locales/de.json'
import ua from './locales/ua.json'

i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    de: { translation: de },
    ua: { translation: ua }
  },
  lng: 'ua',
  fallbackLng: 'en',
  interpolation: {
    escapeValue: false
  }
})

export default i18n
