import{readFile,writeFile as write}from'node:fs/promises';
import*as path from'node:path';

const value = { items: [1, 2, 3], method(input) { return input ?? this.items[0]; } };
