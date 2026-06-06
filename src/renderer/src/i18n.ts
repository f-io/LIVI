import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import de from './locales/de.json'
import en from './locales/en.json'
import ua from './locales/ua.json'
import fr from './locales/fr.json'

i18n.use(initReactI18next).init({
  debug: false,
  resources: {
    en: { translation: en }, // English
    de: { translation: de }, // German
    ua: { translation: ua }, // backward compatibility
    uk: { translation: ua }, // official ISO code (generic)
    fr: { transalation: fr}, // French
    'uk-UA': { translation: ua } // official ISO code (region)
  },
  lng: 'en',
  fallbackLng: 'en',
  supportedLngs: ['en', 'de', 'ua', 'fr', 'uk', 'uk-UA'],
  nonExplicitSupportedLngs: true,
  interpolation: { escapeValue: false }
})

export default i18n
