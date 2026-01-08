import { styled } from '@mui/material/styles'
import Paper from '@mui/material/Paper'
import ArrowForwardIosOutlinedIcon from '@mui/icons-material/ArrowForwardIosOutlined'
import { StackItemProps } from '../../type'

const Item = styled(Paper)(({ theme }) => {
  const activeColor = theme.palette.primary.main

  const rowPad = 'clamp(10px, 1.9svh, 16px)'
  const rowFont = 'clamp(0.9rem, 2.2svh, 1rem)'
  const rowGap = 'clamp(0.75rem, 2.6svh, 3rem)'

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

    '&:hover': {
      borderBottom: `2px solid ${activeColor}`,
      a: { color: activeColor },
      svg: { right: '3px', color: activeColor }
    },
    '&:active': {
      borderBottom: `2px solid ${activeColor}`,
      a: { color: activeColor },
      svg: { right: '3px', color: activeColor }
    },
    '&:focus': {
      borderBottom: `2px solid ${activeColor}`,
      a: { color: activeColor },
      svg: { right: '3px', color: activeColor }
    },

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

      '&:hover': {
        color: activeColor,
        '+ svg': { right: '3px', color: activeColor }
      },
      '&:active': {
        color: activeColor,
        '+ svg': { right: '3px', color: activeColor }
      },
      '&:focus': {
        color: activeColor,
        '+ svg': { right: '3px', color: activeColor }
      }
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
  const viewValue = node?.valueTransform?.toView ? node?.valueTransform.toView(value) : value

  let displayValue = node?.valueTransform?.format
    ? node.valueTransform.format(viewValue)
    : `${viewValue}${node?.displayValueUnit ?? ''}`

  if (node?.type === 'select') {
    const option = node?.options.find((o) => o.value === value)
    displayValue = option?.label || ''
  }

  if (displayValue === 'null' || displayValue === 'undefined') {
    displayValue = '---'
  }

  return (
    <Item onClick={onClick}>
      {children}
      {showValue && (value || displayValue) && (
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
