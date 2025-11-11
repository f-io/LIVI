import { themeColors } from '../../../themeColors'

export const highlightEditableField = ({
  isActive,
  isDarkMode
}: {
  isActive: boolean
  isDarkMode: boolean
}) => {
  if (!isActive) return {}

  const borderColor = isDarkMode
    ? themeColors.highlightEditableFieldDark
    : themeColors.highlightEditableFieldLight

  console.log(isActive, borderColor)

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
          }
        }
      : {})
  }
}
