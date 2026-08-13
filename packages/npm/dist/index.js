import { loadBinding } from './binding.js';
export async function format(fileName, sourceText, config = {}) {
    const configJson = JSON.stringify(config);
    try {
        return await loadBinding().format(fileName, sourceText, configJson);
    }
    catch (error) {
        if (error instanceof Error) {
            const match = /^\[([A-Z_]+)\]\s*/.exec(error.message);
            if (match) {
                Object.defineProperty(error, 'code', {
                    configurable: true,
                    enumerable: true,
                    value: match[1]
                });
                error.message = error.message.slice(match[0].length);
            }
        }
        throw error;
    }
}
