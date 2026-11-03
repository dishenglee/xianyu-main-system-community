import { RouterProvider } from 'react-router-dom'
import { router } from './router'
import { NotifyProvider } from './components/Notify'
import { AppDialogProvider } from './components/AppDialog'

export default function App() {
  return (
    <NotifyProvider>
      <AppDialogProvider>
        <RouterProvider router={router} />
      </AppDialogProvider>
    </NotifyProvider>
  )
}
