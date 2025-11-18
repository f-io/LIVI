import { themeColors } from '../../../themeColors'
import { useCarplayStore } from '@store/store'

export const highlightEditableField = ({
  isActive,
  isDarkMode
}: {
  isActive: boolean
  isDarkMode: boolean
}) => {
  if (!isActive) return {}

  const settings = useCarplayStore.getState().settings
  const override = isDarkMode
    ? settings?.highlightEditableFieldDark
    : settings?.highlightEditableFieldLight

  const borderColor =
    override ??
    (isDarkMode ? themeColors.highlightEditableFieldDark : themeColors.highlightEditableFieldLight)

  return {
    ...(isActive
      ? {
          '& .MuiOutlinedInput-root .MuiOutlinedInput-notchedOutline': {
            borderColor: `${borderColor} !important`
          },
          '& .MuiOutlinedInput-root.Mui-focused .MuiOutlinedInput-notchedOutline': {
            borderColor: `${borderColor} !important`
          },
          '& .MuiOutlinedInput-root.MuiSelect-outlined .MuiOutlinedInput-notchedOutline': {
            borderColor: `${borderColor} !important`
          },
          '& .MuiOutlinedInput-root.MuiSelect-outlined.Mui-focused .MuiOutlinedInput-notchedOutline':
            {
              borderColor: `${borderColor} !important`
            },
          '& .MuiOutlinedInput-root': {
            '& fieldset': {
              borderColor: `${borderColor} !important`
            },
            '&.Mui-focused fieldset': {
              borderColor: `${borderColor} !important`
            },
            '&.MuiSelect-outlined fieldset': {
              borderColor: `${borderColor} !important`
            }
          },
          '& .MuiSlider-thumb': {
            border: `2px solid ${borderColor}`
          },
          '& .MuiSlider-thumb.Mui-focusVisible': {
            outline: `2px solid ${borderColor}`,
            outlineOffset: 4
          },
          '& .MuiSlider-track': {
            borderColor: `${borderColor}`,
            borderWidth: 2
          }
        }
      : {})
  }
}
