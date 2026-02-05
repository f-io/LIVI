import * as React from 'react'
import { useContext } from 'react'
import { AppContext } from '../../../../../context'
import { NumberField as BaseNumberField } from '@base-ui/react/number-field'
import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import FormControl from '@mui/material/FormControl'
import FormLabel from '@mui/material/FormLabel'
import AddIcon from '@mui/icons-material/Add'
import RemoveIcon from '@mui/icons-material/Remove'
import OutlinedInput from '@mui/material/OutlinedInput'
import OpenInFullIcon from '@mui/icons-material/OpenInFull'
import { useTheme, alpha } from '@mui/material/styles'

export default function NumberSpinner({
  id: idProp,
  label,
  error,
  size = 'medium',
  isSlider = false,
  enableScrub = false,
  ...other
}: BaseNumberField.Root.Props & {
  label?: React.ReactNode
  size?: 'small' | 'medium'
  isSlider?: boolean
  enableScrub?: boolean
  error?: boolean
}) {
  const theme = useTheme()

  const appContext = useContext(AppContext)
  const fieldHeight = size === 'small' ? 72 : 88
  const radius = theme.shape.borderRadius

  let id = React.useId()
  if (idProp) {
    id = idProp
  }

  const pressStyles = {
    boxShadow: `inset 1px 0 0 ${theme.palette.primary.main}`,
    borderColor: theme.palette.primary.main
  } as const

  const pressStylesDec = {
    boxShadow: `inset 1px 0 0 ${theme.palette.primary.main}, inset 0 1px 0 ${theme.palette.primary.main}`,
    borderColor: theme.palette.primary.main
  } as const

  return (
    <BaseNumberField.Root
      {...other}
      render={(props, state) => (
        <FormControl
          size={size}
          ref={props.ref}
          disabled={state.disabled}
          required={state.required}
          error={error}
          variant="outlined"
          sx={{
            '& .MuiButton-root': {
              borderColor: 'divider',
              minWidth: 0,
              minHeight: 0,
              padding: 0,
              bgcolor: 'action.hover',
              '&:not(.Mui-disabled)': { color: 'text.primary' }
            },
            width: '100%'
          }}
        >
          {props.children}
        </FormControl>
      )}
    >
      {enableScrub ? (
        <BaseNumberField.ScrubArea
          render={<Box component="span" sx={{ userSelect: 'none', width: 'max-content' }} />}
        >
          <FormLabel
            htmlFor={id}
            sx={{
              display: 'inline-block',
              cursor: 'ew-resize',
              fontSize: '0.875rem',
              color: 'text.primary',
              fontWeight: 500,
              lineHeight: 1.5,
              mb: 0.5
            }}
          >
            {label}
          </FormLabel>
          <BaseNumberField.ScrubAreaCursor>
            <OpenInFullIcon
              fontSize="small"
              sx={{ transform: 'translateY(12.5%) rotate(45deg)' }}
            />
          </BaseNumberField.ScrubAreaCursor>
        </BaseNumberField.ScrubArea>
      ) : (
        <FormLabel
          htmlFor={id}
          sx={{
            display: 'inline-block',
            fontSize: '0.875rem',
            color: 'text.primary',
            fontWeight: 500,
            lineHeight: 1.5,
            mb: 0.5
          }}
        >
          {label}
        </FormLabel>
      )}

      <Box sx={{ display: 'flex', width: '100%' }}>
        {/* Value / input */}
        <div style={{ position: 'relative', width: '100%' }}>
          <BaseNumberField.Input
            id={id}
            render={(props, state) => {
              const editingId = appContext?.keyboardNavigation?.focusedElId ?? null
              const isArmed = editingId === id

              return (
                <OutlinedInput
                  inputRef={props.ref}
                  value={state.inputValue}
                  onBlur={props.onBlur}
                  onChange={props.onChange}
                  onKeyUp={props.onKeyUp}
                  onKeyDown={props.onKeyDown}
                  onFocus={props.onFocus}
                  inputProps={{ readOnly: isSlider }}
                  slotProps={{
                    input: {
                      ...props,

                      id,
                      'aria-label': id,

                      size:
                        Math.max(
                          (other.min?.toString() || '').length,
                          state.inputValue.length || 1
                        ) + 1,
                      sx: {
                        textAlign: 'center',
                        bgcolor: theme.palette.background.paper,
                        caretColor: isSlider ? 'transparent' : 'auto',
                        fontSize:
                          fieldHeight >= 80 ? '1.6rem' : fieldHeight >= 64 ? '1.35rem' : '1.1rem',
                        fontWeight: 500,
                        lineHeight: 1
                      }
                    }
                  }}
                  sx={{
                    pr: 0,
                    height: fieldHeight,
                    borderTopLeftRadius: radius,
                    borderBottomLeftRadius: radius,
                    borderTopRightRadius: 0,
                    borderBottomRightRadius: 0,
                    flex: 1,
                    width: '100%',
                    '& .MuiOutlinedInput-notchedOutline': {
                      borderColor: theme.palette.divider,
                      borderWidth: '1px'
                    },
                    '&:hover .MuiOutlinedInput-notchedOutline': {
                      borderColor: theme.palette.text.secondary,
                      borderWidth: '1px'
                    },
                    '&.Mui-focused .MuiOutlinedInput-notchedOutline': {
                      borderColor: theme.palette.primary.main,
                      borderWidth: '1px'
                    },
                    '& .MuiOutlinedInput-input': {
                      color: isArmed ? theme.palette.primary.main : theme.palette.text.primary
                    }
                  }}
                />
              )
            }}
          />

          {isSlider && (
            <div
              style={{
                position: 'absolute',
                height: 'calc(100% - 2px)',
                top: '1px',
                left: '1px',
                width: `calc(${other.value}% - 2px)`,
                maxWidth: '100%',
                background: alpha(theme.palette.primary.main, 0.25),
                borderTopLeftRadius: radius,
                borderBottomLeftRadius: radius
              }}
            />
          )}
        </div>

        {/* Vertical +/- buttons */}
        <Box
          sx={{
            display: 'flex',
            flexDirection: 'column',
            width: Math.max(fieldHeight, 64),
            minWidth: Math.max(fieldHeight, 64),
            height: fieldHeight
          }}
        >
          <BaseNumberField.Increment
            render={
              <Button
                variant="outlined"
                aria-label="Increase"
                size={size}
                disableFocusRipple
                sx={{
                  height: '50%',
                  minHeight: 0,
                  px: 0,
                  py: 0,
                  lineHeight: 1,
                  borderLeft: '0px',
                  borderTopLeftRadius: 0,
                  borderBottomLeftRadius: 0,
                  borderBottomRightRadius: 0,
                  borderTopRightRadius: radius,
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  '&.Mui-disabled': { borderLeft: '0px' },

                  // Keyboard/D-Pad focus look
                  '&.Mui-focusVisible, &:focus-visible': {
                    ...pressStyles,
                    backgroundColor: alpha(theme.palette.primary.main, 0.12)
                  },
                  '&.Mui-focusVisible .MuiSvgIcon-root, &:focus-visible .MuiSvgIcon-root': {
                    color: theme.palette.primary.main
                  },

                  // Mouse hover
                  '&:hover': pressStyles,
                  '&:hover .MuiSvgIcon-root': { color: theme.palette.primary.main },

                  // Press (mouse + touch) solange gedrückt
                  '&:active': pressStyles,
                  '&:active .MuiSvgIcon-root': { color: theme.palette.primary.main }
                }}
              />
            }
          >
            <AddIcon fontSize={size === 'small' ? 'medium' : 'large'} />
          </BaseNumberField.Increment>

          <BaseNumberField.Decrement
            render={
              <Button
                variant="outlined"
                aria-label="Decrease"
                size={size}
                disableFocusRipple
                sx={{
                  height: '50%',
                  minHeight: 0,
                  px: 0,
                  py: 0,
                  lineHeight: 1,
                  borderLeft: '0px',
                  borderTop: '0px',
                  borderTopLeftRadius: 0,
                  borderBottomLeftRadius: 0,
                  borderTopRightRadius: 0,
                  borderBottomRightRadius: radius,
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  '&.Mui-disabled': { borderLeft: '0px', borderTop: '0px' },

                  // Keyboard/D-Pad focus look
                  '&.Mui-focusVisible, &:focus-visible': {
                    ...pressStylesDec,
                    backgroundColor: alpha(theme.palette.primary.main, 0.12),
                    borderTop: `1px solid ${theme.palette.primary.main}`
                  },
                  '&.Mui-focusVisible .MuiSvgIcon-root, &:focus-visible .MuiSvgIcon-root': {
                    color: theme.palette.primary.main
                  },

                  // Mouse hover
                  '&:hover': {
                    ...pressStylesDec,
                    borderTop: `1px solid ${theme.palette.primary.main}`
                  },
                  '&:hover .MuiSvgIcon-root': { color: theme.palette.primary.main },

                  // Press (mouse + touch) solange gedrückt
                  '&:active': {
                    ...pressStylesDec,
                    borderTop: `1px solid ${theme.palette.primary.main}`
                  },
                  '&:active .MuiSvgIcon-root': { color: theme.palette.primary.main }
                }}
              />
            }
          >
            <RemoveIcon fontSize={size === 'small' ? 'medium' : 'large'} />
          </BaseNumberField.Decrement>
        </Box>
      </Box>
    </BaseNumberField.Root>
  )
}
