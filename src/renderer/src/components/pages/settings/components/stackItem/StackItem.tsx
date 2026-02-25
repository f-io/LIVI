import { styled } from '@mui/material/styles'
import Paper from '@mui/material/Paper'
import ArrowForwardIosOutlinedIcon from '@mui/icons-material/ArrowForwardIosOutlined'
import { StackItemProps } from '../../type'
import React from 'react'
import { useTranslation } from 'react-i18next'

const Item = styled(Paper)(({ theme }) => {
  const activeColor = theme.palette.primary.main

  const rowPad = 'clamp(10px, 1.9svh, 16px)'
  const rowFont = 'clamp(0.9rem, 2.2svh, 1rem)'
  const rowGap = 'clamp(0.75rem, 2.6svh, 3rem)'

  const activeRowStyles = {
    borderBottom: `2px solid ${activeColor}`,
    a: { color: activeColor },
    svg: { right: '3px', color: activeColor }
  } as const

  return {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    flexDirection: 'row',
    gap: rowGap,
    paddingRight: rowPad,
    borderBottom: `2px solid ${theme.palette.divider}`,
    fontSize: rowFont,

    '& svg': {
      position: 'relative',
      right: 0,
      transition: 'all 0.3s ease-in-out'
    },

    // Hover ONLY for real mouse (prevents sticky hover after touch)
    'html[data-input="mouse"] &': {
      '&:hover': activeRowStyles
    },

    // Press feedback (mouse + touch) - same as keyboard highlight
    '&:active': activeRowStyles,

    // Keyboard/D-pad highlight
    '&:focus-visible': {
      outline: 'none',
      ...activeRowStyles
    },

    // IMPORTANT: do not use :focus styling (can stick on touch/click)
    '&:focus': { outline: 'none' },

    ...theme.applyStyles('dark', {
      backgroundColor: 'transparent'
    }),

    '& > p': {
      display: 'flex',
      alignItems: 'center',
      width: '100%',
      padding: rowPad,
      textDecoration: 'none',
      fontSize: rowFont,
      outline: 'none',
      color: theme.palette.text.secondary,
      margin: 0
    },

    '& > a': {
      display: 'flex',
      alignItems: 'center',
      width: '100%',
      padding: rowPad,
      textDecoration: 'none',
      fontSize: rowFont,
      outline: 'none',
      color: theme.palette.text.secondary,

      // Hover ONLY for real mouse
      'html[data-input="mouse"] &': {
        '&:hover': {
          color: activeColor,
          '+ svg': { right: '3px', color: activeColor }
        }
      },

      // Press feedback (mouse + touch) - same as keyboard highlight
      '&:active': {
        color: activeColor,
        '+ svg': { right: '3px', color: activeColor }
      },

      // Keyboard highlight
      '&:focus-visible': {
        color: activeColor,
        '+ svg': { right: '3px', color: activeColor }
      },

      '&:focus': { outline: 'none' }
    }
  }
})

export const StackItem = ({
  children,
  value,
  node,
  showValue,
  withForwardIcon,
  onClick
}: StackItemProps) => {
  const { t } = useTranslation()

  const viewValue = node?.valueTransform?.toView ? node?.valueTransform.toView(value) : value

  let displayValue = node?.valueTransform?.format
    ? node.valueTransform.format(viewValue)
    : `${viewValue}${node?.displayValueUnit ?? ''}`

  if (node?.type === 'select') {
    const option = node?.options.find((o) => o.value === value)
    displayValue = option ? (option.labelKey ? t(option.labelKey, option.label) : option.label) : ''
  }

  if (displayValue === 'null' || displayValue === 'undefined') {
    displayValue = '---'
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (!onClick) return
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      e.stopPropagation()
      onClick()
    }
  }

  return (
    <Item
      onClick={onClick}
      onKeyDown={handleKeyDown}
      tabIndex={onClick ? 0 : -1}
      role={onClick ? 'button' : undefined}
    >
      {children}
      {showValue && value != null && (
        <div style={{ whiteSpace: 'nowrap', fontSize: 'clamp(0.85rem, 2.0svh, 0.95rem)' }}>
          {displayValue}
        </div>
      )}
      {withForwardIcon && (
        <ArrowForwardIosOutlinedIcon
          sx={{ color: 'inherit', fontSize: 'clamp(18px, 3.2svh, 28px)' }}
        />
      )}
    </Item>
  )
}
