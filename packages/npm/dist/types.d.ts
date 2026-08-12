export interface FormatConfig {
    $schema?: string | null;
    ignorePatterns?: string[];
    lineWidth?: number;
    rules?: RulesConfig;
    verifyAst?: boolean;
}
export interface RulesConfig {
    importLayout?: boolean;
    statementSpacing?: StatementSpacingConfig;
}
export interface StatementSpacingConfig {
    imports?: 'separate' | 'compact' | 'off';
    variableDeclarations?: 'separate' | 'compact' | 'off';
}
