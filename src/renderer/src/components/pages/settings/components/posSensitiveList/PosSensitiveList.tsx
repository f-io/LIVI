import ChevronRightIcon from '@mui/icons-material/ChevronRight'
import ExpandLessIcon from '@mui/icons-material/ExpandLess'
import ExpandMoreIcon from '@mui/icons-material/ExpandMore'
import { Box, IconButton, Typography } from '@mui/material'
import type { PosListNode } from '@renderer/routes/types'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { StackItem } from '../stackItem'

type SlotWithPos = { pos?: number } & Record<string, unknown>
type PosMap = Record<string, SlotWithPos>

type Props = {
  node: PosListNode
  value: unknown
  onChange: (next: PosMap) => void
  onItemClick?: (route: string) => void
}

const iconSx = { fontSize: 'clamp(22px, 4.2vh, 34px)' } as const
const btnSx = { padding: 'clamp(4px, 1.2vh, 10px)' } as const

const isPosMap = (v: unknown): v is PosMap =>
  typeof v === 'object' && v !== null && !Array.isArray(v)

// Returns items in their persisted order, fixing duplicate / missing pos
// values along the way so two entries can never share the same slot.
const orderedItems = (node: PosListNode, raw: unknown) => {
  const map: PosMap = isPosMap(raw) ? raw : {}
  return node.items
    .map((item, idx) => {
      const slot = map[item.id]
      const persisted = typeof slot?.pos === 'number' && Number.isFinite(slot.pos) ? slot.pos : null
      return {
        item,
        slot: slot ?? {},
        pos: persisted ?? idx + 1
      }
    })
    .sort((a, b) => a.pos - b.pos)
    .map((entry, idx) => ({ ...entry, pos: idx + 1 }))
}

export const PosSensitiveList = ({ node, value, onChange, onItemClick }: Props) => {
  const { t } = useTranslation()

  const ordered = useMemo(() => orderedItems(node, value), [node, value])

  const swap = (idxA: number, idxB: number) => {
    if (idxA < 0 || idxB < 0 || idxA >= ordered.length || idxB >= ordered.length) return

    const next: PosMap = isPosMap(value) ? { ...value } : {}
    ordered.forEach((entry, i) => {
      let pos = i + 1
      if (i === idxA) pos = idxB + 1
      else if (i === idxB) pos = idxA + 1
      next[entry.item.id] = { ...entry.slot, pos }
    })
    onChange(next)
  }

  return (
    <>
      {ordered.map((entry, index) => {
        const { item } = entry
        const label = item.labelKey ? t(item.labelKey, item.label) : item.label
        const target = item.route ?? item.id
        const canUp = index > 0
        const canDown = index < ordered.length - 1

        return (
          <StackItem key={item.id} onClick={onItemClick ? () => onItemClick(target) : undefined}>
            <Typography>{label}</Typography>

            <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5 }}>
              <IconButton
                sx={btnSx}
                disabled={!canUp}
                onClick={(e) => {
                  e.stopPropagation()
                  swap(index, index - 1)
                }}
              >
                <ExpandLessIcon sx={iconSx} />
              </IconButton>
              <IconButton
                sx={btnSx}
                disabled={!canDown}
                onClick={(e) => {
                  e.stopPropagation()
                  swap(index, index + 1)
                }}
              >
                <ExpandMoreIcon sx={iconSx} />
              </IconButton>
              {onItemClick && <ChevronRightIcon sx={iconSx} />}
            </Box>
          </StackItem>
        )
      })}
    </>
  )
}
