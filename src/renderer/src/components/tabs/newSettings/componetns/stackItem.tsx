import { styled } from '@mui/material/styles'
import Paper from '@mui/material/Paper'
import ArrowForwardIosOutlinedIcon from '@mui/icons-material/ArrowForwardIosOutlined'
import { themeColors } from '../../../../themeColors'

const Item = styled(Paper)(({ theme }) => ({
  ...theme.typography.body2,
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  flexDirection: 'row',
  paddingRight: theme.spacing(2),
  borderBottom: `2px solid ${theme.palette.divider}`,

  '& svg': {
    position: 'relative',
    right: 0,
    transition: 'all 0.3s ease-in-out'
  },

  '&:hover': {
    borderBottom: `2px solid ${themeColors.highlightDark}`,
    a: {
      color: themeColors.highlightDark
    },
    svg: {
      right: '3px',
      color: themeColors.highlightDark
    }
  },
  '&:active': {
    borderBottom: `2px solid ${themeColors.highlightDark}`,
    a: {
      color: themeColors.highlightDark
    },
    svg: {
      right: '3px',
      color: themeColors.highlightDark
    }
  },
  '&:focus': {
    borderBottom: `2px solid ${themeColors.highlightDark}`,
    a: {
      color: themeColors.highlightDark
    },
    svg: {
      right: '3px',
      color: themeColors.highlightDark
    }
  },

  ...theme.applyStyles('dark', {
    backgroundColor: 'transparent'
  }),
  '& > p': {
    display: 'flex',
    alignItems: 'center',
    width: '100%',
    padding: theme.spacing(2),
    textDecoration: 'none',
    fontSize: '1rem',
    outline: 'none',
    color: theme.palette.text.secondary
  },
  '& > a': {
    display: 'flex',
    alignItems: 'center',
    width: '100%',
    padding: theme.spacing(2),
    textDecoration: 'none',
    fontSize: '1rem',
    outline: 'none',
    color: theme.palette.text.secondary,

    // TODO duplicate - need to resolve with keyboard navigation
    '&:hover': {
      color: themeColors.highlightDark,

      '+ svg': {
        right: '3px',
        color: themeColors.highlightDark
      }
    },
    '&:active': {
      color: themeColors.highlightDark,

      '+ svg': {
        right: '3px',
        color: themeColors.highlightDark
      }
    },
    '&:focus': {
      color: themeColors.highlightDark,

      '+ svg': {
        right: '3px',
        color: themeColors.highlightDark
      }
    }
  }
}))

export const StackItem = ({ children, withForwardIcon, onClick }) => {
  return (
    <Item onClick={onClick}>
      {children}
      {withForwardIcon && <ArrowForwardIosOutlinedIcon sx={{ color: 'inherit' }} />}
    </Item>
  )
}
