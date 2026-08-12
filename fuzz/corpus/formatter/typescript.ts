import type{Config,Result as ImportResult}from'package-with-a-long-name';
import{type Value as ImportedValue,createValue}from'package';

interface Value<T> { readonly item: T; }
const value = { item: 'text' } satisfies Value<string>;
