type Nested<T> = ((input: T) => void) | { state: 'idle' | 'loading'; value: T | undefined } | [T | null, (() => void) | undefined]
type Grouped = ((string | number)) | ((boolean | bigint))
type Conditional<T> = T extends string | number ? Promise<'loaded' | 'pending'> : ('empty' | 'error')[]
type Comments = 'before' | /* next | variant */ 'after' | // trailing
  'last'
type Protected = 'before' |
// @ts-expect-error
  Missing | 'after'
