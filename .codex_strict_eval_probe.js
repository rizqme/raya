"use strict";
function testcase() {
  var x = 0;
  function inner() {
    eval("var x = 1");
    throw new Error(JSON.stringify([x, typeof x]));
  }
  inner();
}
testcase();
