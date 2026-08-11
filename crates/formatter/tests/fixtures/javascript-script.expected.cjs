/* script families */
var value = 1;
with ({ value: 2 }) {
  value += 1;
}
label: {
  if (value)
    break label;
}
function inspect(input) {
  %DebugPrint(input);
  return input;
}
const factory = function named(value) {
  return class Inner {
    field = value;
  };
};
new.target;
this.value = value;
