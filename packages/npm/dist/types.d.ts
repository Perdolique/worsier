export interface FormatConfig {
    $schema?: string | null;
    arrays?: ArrayConfig;
    arrowParentheses?: 'always' | 'asNeeded';
    bracketSpacing?: boolean;
    finalNewline?: boolean;
    ignorePatterns?: string[];
    imports?: ImportConfig;
    indentStyle?: 'space' | 'tab';
    indentWidth?: number;
    lineEnding?: 'preserve' | 'lf' | 'crlf';
    lineWidth?: number;
    objects?: ObjectConfig;
    quoteStyle?: 'single' | 'double';
    semicolons?: 'always' | 'asNeeded';
    statementSpacing?: StatementSpacingRule[];
    trailingCommas?: 'none' | 'multiline' | 'all';
    verifyAst?: boolean;
}
export interface ArrayConfig {
    elementLayout?: 'auto' | 'preserve' | 'onePerLine';
    layout?: 'auto' | 'preserve' | 'singleLine' | 'multiLine';
    objectElements?: 'inherit' | 'onePerLine';
}
export interface ImportConfig {
    specifierLayout?: 'auto' | 'preserve' | 'onePerLine';
}
export interface ObjectConfig {
    layout?: 'auto' | 'preserve' | 'singleLine' | 'multiLine';
    propertyLayout?: 'auto' | 'preserve' | 'onePerLine';
    whenArrayElement?: 'inherit' | 'multiLine';
}
export interface StatementSpacingRule {
    blankLines: number;
    next: StatementSelector;
    previous: StatementSelector;
    scope?: 'any' | 'topLevel' | 'block';
}
export interface StatementSelector {
    kind?: 'any' | 'import' | 'export' | 'const' | 'let' | 'var' | 'function' | 'class' | 'type' | 'interface' | 'enum' | 'namespace' | 'other';
    lineShape?: 'any' | 'singleLine' | 'multiLine';
}
