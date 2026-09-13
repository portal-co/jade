/*---
description: throw + try/catch bind the exception value, including across a nested call
flags: [raw]
---*/

// Basic throw/catch binding.
var caught;
try {
  throw 42;
} catch (e) {
  caught = e;
}
if (caught !== 42) return 1;

// Statements after the throwing statement do not run.
var ran = false;
try {
  throw 1;
  ran = true;
} catch (e) {}
if (ran !== false) return 2;

// Normal completion runs no handler.
var handlerRan = false;
try {
  var ok = 1;
} catch (e) {
  handlerRan = true;
}
if (handlerRan !== false) return 3;
if (ok !== 1) return 4;

// An exception thrown by a callee propagates through CALL to the caller's
// innermost region.
function throws() {
  throw 7;
}
var fromCall;
try {
  throws();
} catch (e) {
  fromCall = e;
}
if (fromCall !== 7) return 5;

// An uncaught exception inside a nested function propagates out of that
// function's frame to the caller's handler.
function inner() {
  try {
    throw 9;
  } catch (e) {
    throw e;
  }
}
var propagated;
try {
  inner();
} catch (e) {
  propagated = e;
}
if (propagated !== 9) return 6;

return 0;
