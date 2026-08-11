import { loadBinding } from './binding.js'
import type { FormatConfig } from './types.js'

export type { FormatConfig } from './types.js'

export async function format(fileName: string, sourceText: string, config: FormatConfig): Promise<string> {
  const configJson = JSON.stringify(config)
  try {
    return await loadBinding().format(fileName, sourceText, configJson)
  } catch (error) {
    if (error instanceof Error) {
      const match = /^\[([A-Z_]+)\]\s*/.exec(error.message)
      if (match) {
        Object.defineProperty(error, 'code', {
          configurable: true,
          enumerable: true,
          value: match[1]
        })
        error.message = error.message.slice(match[0].length)
      }
    }
    throw error
  }
}
