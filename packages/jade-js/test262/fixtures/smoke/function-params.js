/*---
description: Declared function parameters bind call arguments (FN opcode params operand)
flags: [raw]
---*/

// Declaration form.
function id(x) {
  return x;
}
if (id(42) !== 42) return 1;

// Expression form, two parameters, order matters.
function pair(a, b) {
  return a === b;
}
if (pair(7, 7) === false) return 2;
if (pair(7, 8) === true) return 3;

// A missing argument binds undefined.
function second(a, b) {
  return b;
}
if (second(1) !== undefined) return 4;

// Extra arguments are dropped.
if (id(11, 22, 33) !== 11) return 5;

// Parameters are per-call: no leakage between invocations.
function tag(x) {
  return x;
}
tag(5);
if (tag(6) !== 6) return 6;

// Parameters still bind when the program also references the global object
// (the frontend then reserves state slot 0 for the GLOBAL op).
function pick(x) {
  return x;
}
var g = globalThis;
if (pick(9) !== 9) return 7;

return 0;
