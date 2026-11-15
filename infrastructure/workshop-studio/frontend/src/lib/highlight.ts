import hljs from 'highlight.js/lib/core'
import sql from 'highlight.js/lib/languages/sql'
import yaml from 'highlight.js/lib/languages/yaml'
import python from 'highlight.js/lib/languages/python'

hljs.registerLanguage('sql', sql)
hljs.registerLanguage('yaml', yaml)
hljs.registerLanguage('python', python)

const ALIAS: Record<string, string> = { 'jinja-sql': 'sql', jinja: 'sql', yml: 'yaml', py: 'python' }

export function escapeHtml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

/**
 * Highlights the whole block (so multi-line comments and strings stay right), then splits the HTML per line,
 * closing the open <span>s at each line end and re-opening them on the next line.
 */
const MASK = /•{3,}/g
/** A masked secret (••••••) stays readable and says why it is hidden. Only text is touched: the bullets never occur inside a tag. */
export function markMasks(html: string): string {
  return html.replace(MASK, (m) => `<span class="code-mask" title="Hidden on purpose">${m}</span>`)
}

export function highlightLines(lines: string[], language: string): string[] {
  return highlightRaw(lines, language).map(markMasks)
}

function highlightRaw(lines: string[], language: string): string[] {
  const lang = ALIAS[language] ?? language
  if (!hljs.getLanguage(lang)) return lines.map(escapeHtml)
  let html: string
  try {
    html = hljs.highlight(lines.join('\n'), { language: lang, ignoreIllegals: true }).value
  } catch {
    return lines.map(escapeHtml)
  }
  const out: string[] = []
  const open: string[] = []
  let cur = ''
  for (const part of html.split(/(<[^>]+>|\n)/)) {
    if (part === '\n') {
      out.push(cur + '</span>'.repeat(open.length))
      cur = open.join('')
    } else if (part.startsWith('</')) {
      open.pop()
      cur += part
    } else if (part.startsWith('<')) {
      open.push(part)
      cur += part
    } else {
      cur += part
    }
  }
  out.push(cur + '</span>'.repeat(open.length))
  return out
}
