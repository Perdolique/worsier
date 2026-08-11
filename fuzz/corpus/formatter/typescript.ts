interface Value<T> { readonly item: T; }
const value = { item: 'text' } satisfies Value<string>;
