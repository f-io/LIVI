import { SyntheticEvent, useState, ReactNode } from 'react'
import Tabs from '@mui/material/Tabs'
import Tab from '@mui/material/Tab'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import Grid from '@mui/material/Grid'
import BottomNavigation from '@mui/material/BottomNavigation'
import BottomNavigationAction from '@mui/material/BottomNavigationAction'
import { SETTINGS_CONFIG } from './config'

interface TabPanelProps {
  children?: ReactNode
  dir?: string
  index: number
  value: number
}

function TabPanel(props: TabPanelProps) {
  const { children, value, index, ...other } = props

  return (
    <div
      role="tabpanel"
      hidden={value !== index}
      id={`full-width-tabpanel-${index}`}
      aria-labelledby={`full-width-tab-${index}`}
      {...other}
    >
      {value === index && (
        <Box sx={{ p: 3, overflow: 'hidden', height: '70dvh' }}>
          <Box sx={{ height: '100%', overflowX: 'auto', overflowY: 'auto' }}>
            <Typography>{children}</Typography>
          </Box>
        </Box>
      )}
    </div>
  )
}

export const NewSettings = () => {
  const [value, setValue] = useState(0)

  const handleChange = (_: SyntheticEvent, newValue: string) => {
    setValue(+newValue)
  }

  return (
    <Box sx={{ width: '100%' }}>
      <Grid container>
        <Grid size="auto">
          <Box
            sx={{
              flexGrow: 1,
              bgcolor: 'background.paper',
              display: 'flex',
              height: '75dvh'
            }}
          >
            <Tabs
              value={value}
              onChange={handleChange}
              indicatorColor="secondary"
              aria-label="secondary tabs example"
              orientation="vertical"
              scrollButtons="auto"
              variant="scrollable"
            >
              {SETTINGS_CONFIG.map((item, index) => (
                <Tab value={index} label={item.title} key={index} sx={{ alignItems: 'flex-end' }} />
              ))}
            </Tabs>
          </Box>
        </Grid>

        <Grid size={9}>
          <Box sx={{ width: '100%', marginTop: '35px' }}>
            {SETTINGS_CONFIG.map((item, index) => {
              const Component = item.component

              if (value !== index) return null

              return (
                <TabPanel value={index} index={index} key={index}>
                  <Component />
                </TabPanel>
              )
            })}
          </Box>
        </Grid>
      </Grid>
      <BottomNavigation
        value={value}
        sx={{
          position: 'fixed',
          bottom: 0,
          right: '-20px',
          width: '50%',
          justifyContent: 'flex-end'
        }}
      >
        <BottomNavigationAction label="Save" />
      </BottomNavigation>
    </Box>
  )
}
