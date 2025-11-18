import { SettingsLayout } from '@renderer/components/layouts'
import List from '@mui/material/List'
import ListItem from '@mui/material/ListItem'
import ListItemText from '@mui/material/ListItemText'
import Typography from '@mui/material/Typography'

export const About = () => {
  return (
    <SettingsLayout>
      <Typography>pi-carplay</Typography>

      <List>
        <ListItem>
          <ListItemText primary="pi-carplay brings Apple CarPlay and Android Auto to the Raspberry Pi, with support for Linux (ARM/x86) and macOS (ARM) as well. It is a standalone Electron app, optimized for embedded setups and ultra-low-resolution OEM displays." />
        </ListItem>

        <ListItem>
          <ListItemText primary="License: " />
          <ListItemText primary="This project is licensed under the MIT License." />
        </ListItem>
      </List>

      <Typography>Disclaimer</Typography>
      <Typography>
        Apple and CarPlay are trademarks of Apple Inc. Android and Android Auto are trademarks of
        Google LLC. This project is not affiliated with or endorsed by Apple or Google. All product
        names, logos, and brands are the property of their respective owners.
      </Typography>
    </SettingsLayout>
  )
}
