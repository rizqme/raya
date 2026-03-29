function subClass(type) {
  try {
    return new Function('return class My' + type + ' extends ' + type + ' {}')();
  } catch (e) {}
}

const MyUint8Array = subClass('Uint8Array');
const MyFloat32Array = subClass('Float32Array');
const MyBigInt64Array = subClass('BigInt64Array');

const builtinCtors = [
  Uint8Array,
  Int8Array,
  Uint16Array,
  Int16Array,
  Uint32Array,
  Int32Array,
  Float32Array,
  Float64Array,
  Uint8ClampedArray,
];

if (typeof Float16Array !== 'undefined') {
  builtinCtors.push(Float16Array);
}

if (typeof BigUint64Array !== 'undefined') {
  builtinCtors.push(BigUint64Array);
}

if (typeof BigInt64Array !== 'undefined') {
  builtinCtors.push(BigInt64Array);
}

const ctors = builtinCtors.concat(MyUint8Array, MyFloat32Array);

if (typeof MyBigInt64Array !== 'undefined') {
  ctors.push(MyBigInt64Array);
}

function CreateResizableArrayBuffer(byteLength, maxByteLength) {
  return new ArrayBuffer(byteLength, { maxByteLength: maxByteLength });
}

function MayNeedBigInt(ta, n) {
  if ((typeof BigInt64Array !== 'undefined' && ta instanceof BigInt64Array) ||
      (typeof BigUint64Array !== 'undefined' && ta instanceof BigUint64Array)) {
    return BigInt(n);
  }
  return n;
}

function Convert(item) {
  if (typeof item === 'bigint') {
    return Number(item);
  }
  return item;
}

function ToNumbers(array) {
  let result = [];
  for (let i = 0; i < array.length; i++) {
    result.push(Convert(array[i]));
  }
  return result;
}

let out = [];

for (let ctor of ctors) {
  const rab = CreateResizableArrayBuffer(
    4 * ctor.BYTES_PER_ELEMENT,
    8 * ctor.BYTES_PER_ELEMENT
  );
  const fixedLength = new ctor(rab, 0, 4);
  const fixedLengthWithOffset = new ctor(rab, 2 * ctor.BYTES_PER_ELEMENT, 2);
  const lengthTracking = new ctor(rab, 0);
  const lengthTrackingWithOffset = new ctor(rab, 2 * ctor.BYTES_PER_ELEMENT);

  let taWrite = new ctor(rab);
  for (let i = 0; i < 4; ++i) {
    taWrite[i] = MayNeedBigInt(taWrite, i);
  }

  let [a, b, c, d, e] = fixedLength;
  let [f, g, h] = fixedLengthWithOffset;
  let [i, j, k, l, m] = lengthTracking;
  let [n, o, p] = lengthTrackingWithOffset;

  out.push((ctor.name || '<anon>') + '=' + JSON.stringify({
    fixed: ToNumbers([a, b, c, d, e === undefined ? null : e]),
    offset: ToNumbers([f, g, h === undefined ? null : h]),
    track: ToNumbers([i, j, k, l, m === undefined ? null : m]),
    trackOffset: ToNumbers([n, o, p === undefined ? null : p]),
    lengths: [
      fixedLength.length,
      fixedLengthWithOffset.length,
      lengthTracking.length,
      lengthTrackingWithOffset.length,
    ],
  }));
}

throw new Error(out.join('\n'));
