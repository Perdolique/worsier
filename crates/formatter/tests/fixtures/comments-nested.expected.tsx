import {
  first, // import line
  second
} from 'pkg';
export { first, /* export block */ second };
function example(
  first, // parameter line
  second, { value, /* binding block */ other }
) {
  const a = 1, // declarator line
    b = 2;
  call(first, /* argument block */ ...[second]);
  ({ value, /* target block */ other } = source);
  switch (first) {
    case 1:
      break; /* case block */
    case 2:
      return second;
  }
}
const view = <View first={first} // attribute line
  second={second}>{ /* child block */ }<span /></View>;
