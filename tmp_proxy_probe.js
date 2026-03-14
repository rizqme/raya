let newt = new Proxy(function() {}, { get(t, p) { return typeof p; } });
newt["prototype"]
