import { createContext, useContext } from 'react'

/** The two global dialogs, openable from anywhere (header badge, status row, not-found screen). */
export interface Ui {
  openMode: () => void
  openSwitcher: () => void
}
export const UiContext = createContext<Ui>({ openMode: () => undefined, openSwitcher: () => undefined })
export const useUi = (): Ui => useContext(UiContext)
