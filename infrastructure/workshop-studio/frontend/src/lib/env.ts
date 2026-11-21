import type { EnvName } from '../api/types'

/** The names the audience hears. The backend's short codes never reach the screen. */
export const ENV_NAME: Record<EnvName, string> = { dev: 'development', val: 'validatie', prd: 'productie' }
export const ENV_ORDER: EnvName[] = ['dev', 'val', 'prd']

export const envName = (env: string): string => (ENV_NAME as Record<string, string>)[env] ?? env

/** Replaces the whole words dev / val / prd in a sentence from the backend. */
export function envWords(text: string): string {
  return text.replace(/\b(dev|val|prd)\b/g, (m) => ENV_NAME[m as EnvName])
}
