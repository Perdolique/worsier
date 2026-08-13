import type{Config,Result as ImportResult}from'package-with-a-long-name';
import{type Value as ImportedValue,createValue}from'package';

interface Value<T> { readonly item: T; }
const value = { item: 'text' } satisfies Value<string>;

namespace Runtime { const first: Value<string> = value; work(); }

switch (value.item) { case 'text': let result: ImportResult; consume(result); }

type Pair<
  Left,
  Right,
> = [
  Left,
  Right,
];

enum State {
  Ready,
}

interface SemicolonGroups {
  first: string;
  second(): void,
  third: number
}

type OptionalValues<Value> = {
  [Key in keyof Value]?: Value[Key];
};
