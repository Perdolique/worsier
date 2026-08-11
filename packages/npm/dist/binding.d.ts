export interface NativeBinding {
    format(fileName: string, sourceText: string, configJson: string): Promise<string>;
    runCli(args: string[]): Promise<number>;
}
export declare function loadBinding(): NativeBinding;
