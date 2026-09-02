export type InterfaceLayoutMode = 'off';
export type SemicolonMode = 'always' | 'asNeeded' | 'off';
export type StatementSpacingMode = 'separate' | 'compact' | 'off';
export interface FormatConfig {
    $schema?: string | null;
    ignorePatterns?: string[];
    lineWidth?: number;
    rules?: RulesConfig;
    verifyAst?: boolean;
}
export interface RulesConfig {
    commentSpacing?: boolean;
    importLayout?: boolean;
    interfaceLayout?: number | InterfaceLayoutMode;
    objectPropertySpacing?: boolean;
    semicolons?: SemicolonConfig;
    statementSpacing?: StatementSpacingConfig;
    trailingCommas?: 'always' | 'never' | 'off';
}
export interface SemicolonConfig {
    classMembers?: 'always' | 'asNeeded' | 'off';
    statements?: 'always' | 'asNeeded' | 'off';
    typeMembers?: SemicolonMode | TypeMemberSemicolonConfig;
}
export interface TypeMemberSemicolonConfig {
    multiline?: 'always' | 'asNeeded' | 'off';
    singleLine?: 'always' | 'asNeeded' | 'off';
}
export interface StatementSpacingConfig {
    controlFlowStatements?: 'separate' | 'compact' | 'off';
    imports?: 'separate' | 'compact' | 'off';
    multilineCallStatements?: 'separate' | 'compact' | 'off';
    returnStatements?: 'separate' | 'compact' | 'off';
    singleLineCallStatements?: StatementSpacingMode | SingleLineCallStatementSpacingConfig;
    typeAliases?: 'separate' | 'compact' | 'off';
    variableDeclarations?: 'separate' | 'compact' | 'off';
}
export interface SingleLineCallStatementSpacingConfig {
    betweenCalls?: 'separate' | 'compact' | 'off';
    withOtherStatements?: 'separate' | 'compact' | 'off';
}
