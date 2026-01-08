import { Outlet } from 'react-router'

export const Layout = () => {
  return (
    <div id="main-root">
      <Outlet />
    </div>
  )
}
