import { useEffect, useState } from 'react'
import { HashRouter as Router, Route, Routes } from 'react-router-dom'
import { Home, Carplay, Camera, Info, Media, Settings } from './components/tabs'
import Nav from './components/Nav'
import { Box, Modal } from '@mui/material'
import { useCarplayStore, useStatusStore } from './store/store'
import type { KeyCommand } from '@worker/types'
import { updateCameras } from './utils/cameraDetection'

const style = {
  position: 'absolute',
  top: '50%',
  left: '50%',
  transform: 'translate(-50%, -50%)',
  height: '95%',
  width: '95%',
  boxShadow: 24,
  display: 'flex'
}

function broadcastMediaKey(action: string) {
  window.dispatchEvent(new CustomEvent('car-media-key', { detail: { command: action } }))
}

function App() {
  const [receivingVideo, setReceivingVideo] = useState(false)
  const [commandCounter, setCommandCounter] = useState(0)
  const [keyCommand, setKeyCommand] = useState('')

  const reverse = useStatusStore((state) => state.reverse)
  const setReverse = useStatusStore((state) => state.setReverse)

  const settings = useCarplayStore((state) => state.settings)
  const saveSettings = useCarplayStore((state) => state.saveSettings)
  const setCameraFound = useStatusStore((state) => state.setCameraFound)

  useEffect(() => {
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [settings])

  const onKeyDown = (event: KeyboardEvent) => {
    if (!settings) return

    if (Object.values(settings.bindings).includes(event.code)) {
      const action = Object.keys(settings.bindings).find(
        (key) => settings.bindings[key] === event.code
      )
      if (action !== undefined) {
        setKeyCommand(action)
        setCommandCounter((prev) => prev + 1)
        broadcastMediaKey(action)

        if (action === 'selectDown') {
          setTimeout(() => {
            setKeyCommand('selectUp')
            setCommandCounter((prev) => prev + 1)
          }, 200)
        }
      }
    }
  }

  useEffect(() => {
    if (!settings) return

    updateCameras(setCameraFound, saveSettings, settings)

    const usbHandler = (_: any, data: { type: string }) => {
      if (['attach', 'plugged', 'detach', 'unplugged'].includes(data.type)) {
        updateCameras(setCameraFound, saveSettings, settings)
      }
    }

    window.carplay.usb.listenForEvents(usbHandler)
    return () => window.carplay.usb.unlistenForEvents?.(usbHandler)
  }, [settings])

  return (
    <Router>
      <div style={{ height: '100%', touchAction: 'none' }} id="main" className="App">
        <Nav receivingVideo={receivingVideo} settings={settings} />
        {settings && (
          <Carplay
            receivingVideo={receivingVideo}
            setReceivingVideo={setReceivingVideo}
            settings={settings}
            command={keyCommand as KeyCommand}
            commandCounter={commandCounter}
          />
        )}
        <Routes>
          <Route path="/" element={<Home />} />
          <Route path="/media" element={<Media />} />
          <Route path="/settings" element={<Settings settings={settings!} />} />
          <Route path="/info" element={<Info />} />
          <Route path="/camera" element={<Camera settings={settings!} />} />
        </Routes>
        <Modal open={reverse} onClick={() => setReverse(false)}>
          <Box sx={style}>
            <Camera settings={settings} />
          </Box>
        </Modal>
      </div>
    </Router>
  )
}

export default App
