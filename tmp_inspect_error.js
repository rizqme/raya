const c = null;

function inspect(fn) {
  try {
    fn();
    return ["ok"];
  } catch (e) {
    return [
      e && e.name,
      e && e.message,
      e && e.constructor && e.constructor.name,
      e && e.constructor === TypeError,
      e instanceof TypeError,
      TypeError && TypeError.name,
    ];
  }
}

const payload = {
  constArray: inspect(function () {
    0, [c] = [1];
  }),
  constObj: inspect(function () {
    0, ({ c } = { c: 1 });
  }),
  strictMath: inspect(function () {
    "use strict";
    Math.PI = 1;
  }),
};

throw new Error(JSON.stringify(payload));
