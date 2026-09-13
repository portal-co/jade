/*---
description: rethrow from a handler reaches the next enclosing region
flags: [raw]
---*/

// A rethrow from an inner handler is caught by the outer handler.
var caught;
try {
  try {
    throw 5;
  } catch (e) {
    throw 6;
  }
} catch (e2) {
  caught = e2;
}
if (caught !== 6) return 1;

// The inner handler runs before the rethrow propagates.
var innerRan = false;
var outer;
try {
  try {
    throw 1;
  } catch (e) {
    innerRan = true;
    throw 2;
  }
} catch (e2) {
  outer = e2;
}
if (innerRan !== true) return 2;
if (outer !== 2) return 3;

// Sibling regions are independent: a caught exception in the first does not
// disturb the second.
var a;
var b;
try {
  throw 10;
} catch (e) {
  a = e;
}
try {
  throw 20;
} catch (e) {
  b = e;
}
if (a !== 10) return 4;
if (b !== 20) return 5;

// An exception thrown after a region completed normally is not caught by it.
var late = false;
try {
  var done = 1;
} catch (e) {
  late = true;
}
if (done !== 1) return 6;
if (late !== false) return 7;

return 0;
