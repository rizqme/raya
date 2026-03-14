var same = function(a, b) {
  if (a === 0 && b === 0) return 1 / a === 1 / b;
  if (a !== a && b !== b) return true;
  return a === b;
};

var isConfigurable = function(obj, name) {
  try {
    delete obj[name];
  } catch (e) {
    if (!(e instanceof TypeError)) throw new Error("Expected TypeError, got " + String(e));
  }
  return !Object.prototype.hasOwnProperty.call(obj, name);
};

var isEnumerable = function(obj, name) {
  var stringCheck = false;
  for (var x in obj) {
    if (x === name) {
      stringCheck = true;
      break;
    }
  }
  return stringCheck &&
    Object.prototype.hasOwnProperty.call(obj, name) &&
    Object.prototype.propertyIsEnumerable.call(obj, name);
};

var isWritable = function(obj, name) {
  var newValue = "unlikelyValue";
  var hadValue = Object.prototype.hasOwnProperty.call(obj, name);
  var oldValue = obj[name];
  var writeSucceeded = false;
  try {
    obj[name] = newValue;
  } catch (e) {
    if (!(e instanceof TypeError)) throw new Error("Expected TypeError, got " + String(e));
  }
  writeSucceeded = same(obj[name], newValue);
  if (writeSucceeded) {
    if (hadValue) obj[name] = oldValue;
    else delete obj[name];
  }
  return writeSucceeded;
};

var verifyProperty = function(obj, name, desc) {
  var originalDesc = Object.getOwnPropertyDescriptor(obj, name);
  if (desc === undefined) {
    if (originalDesc !== undefined) throw new Error("descriptor should be undefined");
    return "ok-undefined";
  }
  if (!Object.prototype.hasOwnProperty.call(obj, name)) throw new Error("missing own prop");
  if (Object.prototype.hasOwnProperty.call(desc, "value")) {
    if (!same(desc.value, originalDesc.value)) throw new Error("desc value mismatch");
    if (!same(desc.value, obj[name])) throw new Error("obj value mismatch");
  }
  if (Object.prototype.hasOwnProperty.call(desc, "enumerable") && desc.enumerable !== undefined) {
    if (desc.enumerable !== originalDesc.enumerable || desc.enumerable !== isEnumerable(obj, name)) {
      throw new Error("enumerable mismatch");
    }
  }
  if (Object.prototype.hasOwnProperty.call(desc, "writable") && desc.writable !== undefined) {
    if (desc.writable !== originalDesc.writable || desc.writable !== isWritable(obj, name)) {
      throw new Error("writable mismatch");
    }
  }
  if (Object.prototype.hasOwnProperty.call(desc, "configurable") && desc.configurable !== undefined) {
    if (desc.configurable !== originalDesc.configurable || desc.configurable !== isConfigurable(obj, name)) {
      throw new Error("configurable mismatch");
    }
  }
  return "ok";
};

var errors = [];
var message = "my-message";
var cause = { message: "my-cause" };

try {
  var error = new AggregateError(errors, message, { cause: cause });
  var r1 = verifyProperty(error, "cause", {
    configurable: true,
    enumerable: false,
    writable: true,
    value: cause,
  });
  var r2 = verifyProperty(new AggregateError(errors, message), "cause", undefined);
  var r3 = verifyProperty(new AggregateError(errors, message, { cause: undefined }), "cause", {
    value: undefined,
  });
  throw new Error("ok|" + r1 + "|" + r2 + "|" + r3);
} catch (e) {
  throw new Error(
    String(e && e.constructor && e.constructor.name) + "|" + String(e)
  );
}
