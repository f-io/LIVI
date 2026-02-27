import * as React from 'react'

// TODO move to the AppContext and make it global
// TODO remove document.getElementById and other and use a more react-y way to do this (ref, etc)
// TODO Align with code in the AppLayout (mapsNavHidden vs navHidden, etc)
export const useNavbarHidden = () => {
  const [navHidden, setNavHidden] = React.useState(() => {
    const el = document.getElementById('content-root')
    return el?.getAttribute('data-nav-hidden') === '1'
  })

  React.useLayoutEffect(() => {
    const el = document.getElementById('content-root')
    if (!el) return

    console.log(111, el)
    const read = () => setNavHidden(el.getAttribute('data-nav-hidden') === '1')
    read()

    const mo = new MutationObserver(read)
    mo.observe(el, { attributes: true, attributeFilter: ['data-nav-hidden'] })

    return () => mo.disconnect()
  }, [])

  return {
    isNavbarHidden: navHidden,
    onSetNavHidden: setNavHidden
  }
}
