import{readFile,writeFile as write}from'node:fs/promises';
import*as path from'node:path';

const value = { items: [1, 2, 3], method(input) { return input ?? this.items[0]; } };

function nested() { if (value) { const first = value.items[0]; let second = first ?? 0; use(second); } finish(); }

const nestedLists = {
  values: [
    value,
  ],
};

consume(
  nestedLists,
);
