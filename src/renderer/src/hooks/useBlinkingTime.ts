import { useEffect, useState } from 'react'

export function useBlinkingTime() {
  const [now, setNow] = useState(new Date())

  useEffect(() => {
    const i = setInterval(() => setNow(new Date()), 1000)
    return () => clearInterval(i)
  }, [])

  const showColon = now.getSeconds() % 2 === 0

  const time = now.toLocaleTimeString('ru-RU', {
    hour: '2-digit',
    minute: '2-digit',
    hour12: false
  })

  return showColon ? time : time.replace(':', ' ')
}
