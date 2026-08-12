export interface FormatConfig {
    $schema?: string | null;
    ignorePatterns?: string[];
    lineWidth?: number;
    rules?: RulesConfig;
    verifyAst?: boolean;
}
export interface RulesConfig {
    imports?: boolean;
}
