import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const platformPackages = {
    'darwin-arm64': 'worsier-darwin-arm64',
    'darwin-x64': 'worsier-darwin-x64',
    'linux-arm64-gnu': 'worsier-linux-arm64-gnu',
    'linux-arm64-musl': 'worsier-linux-arm64-musl',
    'linux-x64-gnu': 'worsier-linux-x64-gnu',
    'linux-x64-musl': 'worsier-linux-x64-musl',
    'win32-arm64-msvc': 'worsier-win32-arm64-msvc',
    'win32-x64-msvc': 'worsier-win32-x64-msvc'
};
let binding;
function libc() {
    if (process.platform !== 'linux') {
        return 'gnu';
    }
    const report = process.report?.getReport();
    return report?.header?.glibcVersionRuntime ? 'gnu' : 'musl';
}
export function loadBinding() {
    if (binding) {
        return binding;
    }
    let suffix = '';
    if (process.platform === 'linux') {
        suffix = `-${libc()}`;
    }
    else if (process.platform === 'win32') {
        suffix = '-msvc';
    }
    const target = `${process.platform}-${process.arch}${suffix}`;
    const packageName = platformPackages[target];
    if (!packageName) {
        throw new Error(`Worsier does not support ${target}.`);
    }
    try {
        binding = require(packageName);
    }
    catch (error) {
        const reason = error instanceof Error ? error.message : String(error);
        throw new Error(`Failed to load the Worsier native package ${packageName}: ${reason}`, { cause: error });
    }
    return binding;
}
