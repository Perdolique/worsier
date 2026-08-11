import type { FormatConfig } from './types.js';
export type { FormatConfig } from './types.js';
export declare function format(fileName: string, sourceText: string, config: FormatConfig): Promise<string>;
