// Generated from the Rust FormatConfig JSON Schema. Do not edit.

export type InterfaceLayoutMode = 'off'
export type SemicolonMode = 'always' | 'asNeeded' | 'off'

export interface FormatConfig {
  $schema?: string | null
  ignorePatterns?: string[]
  lineWidth?: number
  rules?: RulesConfig
  verifyAst?: boolean
}
export interface RulesConfig {
  importLayout?: boolean
  interfaceLayout?: number | InterfaceLayoutMode
  objectPropertySpacing?: boolean
  semicolons?: SemicolonConfig
  statementSpacing?: StatementSpacingConfig
  trailingCommas?: 'always' | 'never' | 'off'
}
export interface SemicolonConfig {
  classMembers?: 'always' | 'asNeeded' | 'off'
  statements?: 'always' | 'asNeeded' | 'off'
  typeMembers?: SemicolonMode | TypeMemberSemicolonConfig
}
export interface TypeMemberSemicolonConfig {
  multiline?: 'always' | 'asNeeded' | 'off'
  singleLine?: 'always' | 'asNeeded' | 'off'
}
export interface StatementSpacingConfig {
  controlFlowStatements?: 'separate' | 'compact' | 'off'
  imports?: 'separate' | 'compact' | 'off'
  multilineCallStatements?: 'separate' | 'compact' | 'off'
  returnStatements?: 'separate' | 'compact' | 'off'
  typeAliases?: 'separate' | 'compact' | 'off'
  variableDeclarations?: 'separate' | 'compact' | 'off'
}
