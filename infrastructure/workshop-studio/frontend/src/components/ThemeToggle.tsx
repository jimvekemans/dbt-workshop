import { useTheme } from '../lib/theme'
import { IconButton } from './IconButton'
import { MoonIcon, SunIcon } from './icons'

export function ThemeToggle() {
  const { theme, toggle } = useTheme()
  return (
    <IconButton label={theme === 'dark' ? 'Schakel over naar de lichte weergave' : 'Schakel over naar de donkere weergave'} onClick={toggle}>
      {theme === 'dark' ? <SunIcon /> : <MoonIcon />}
    </IconButton>
  )
}
