for (let i = 0; i < 1000; i++) {
  try {
    eval("/*" + String.fromCharCode(i) + "*/");
  } catch (e) {
    throw new Error("iter:" + i + ":" + e.name + ":" + e.message);
  }
}
throw new Error("ok");
