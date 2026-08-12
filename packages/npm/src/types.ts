// Generated from the Rust FormatConfig JSON Schema. Do not edit.

export type StatementSpacingMode = 'separate' | 'compact' | 'off'

export interface FormatConfig {
  $schema?: string | null
  ignorePatterns?: string[]
  lineWidth?: number
  rules?: RulesConfig
  verifyAst?: boolean
}
export interface RulesConfig {
  importLayout?: boolean
  statementSpacing?: StatementSpacingConfig
}
export interface StatementSpacingConfig {
  imports?: StatementSpacingMode
  variableDeclarations?: StatementSpacingMode
}
