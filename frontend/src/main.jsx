import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
// The font comes with the page (no internet at the field).
import '@fontsource/inter/400.css'
import '@fontsource/inter/500.css'
import '@fontsource/inter/600.css'
import './index.css'
import App from './App.jsx'

createRoot(/** @type {HTMLElement} */ (document.getElementById('root'))).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
