(() => {
  let x = 1;
  try {
    eval("/*x*/");
    throw new Error("ok");
  } catch (e) {
    throw new Error(e.name + ":" + e.message);
  }
})();
