import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@fontsource-variable/bricolage-grotesque'
import '@fontsource-variable/jetbrains-mono'
import './styles/tokens.css'
import './styles/base.css'
import './styles/layout.css'
import { createBackend } from './api/client'
import { StudioProvider } from './lib/studio'
import { ToastProvider } from './components/Toast'
import { App } from './App'

async function boot() {
  const backend = await createBackend()
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <ToastProvider>
        <StudioProvider backend={backend}>
          <App />
        </StudioProvider>
      </ToastProvider>
    </StrictMode>,
  )
}
void boot()
