export interface FormatConfig {
    $schema?: string | null;
    ignorePatterns?: string[];
    lineWidth?: number;
    rules?: RulesConfig;
    verifyAst?: boolean;
}
export interface RulesConfig {
    importLayout?: boolean;
    semicolons?: SemicolonConfig;
    statementSpacing?: StatementSpacingConfig;
    trailingCommas?: 'always' | 'never' | 'off';
}
export interface SemicolonConfig {
    classMembers?: 'always' | 'asNeeded' | 'off';
    statements?: 'always' | 'asNeeded' | 'off';
    typeMembers?: 'always' | 'asNeeded' | 'off';
}
export interface StatementSpacingConfig {
    imports?: 'separate' | 'compact' | 'off';
    variableDeclarations?: 'separate' | 'compact' | 'off';
}
