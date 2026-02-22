# POSIX Standard Library

15 modules for system programming on POSIX-compliant platforms (Linux, macOS, BSD).

**Package:** `raya-stdlib-posix`

## fs (File System)

**Import:** `import fs from "std:fs"`

```typescript
// Read/write files
const content = fs.readTextFile("file.txt");
fs.writeTextFile("out.txt", "data");

// Binary I/O
const bytes = fs.readFile("image.png");
fs.writeFile("copy.png", bytes);

// Directory operations
fs.createDir("mydir");
const entries = fs.readDir(".");
fs.removeDir("mydir");

// File info
const info = fs.stat("file.txt");
logger.info(info.size, info.modified);

// Existence check
if (fs.exists("config.json")) {
  // ...
}
```

## net (Networking)

**Import:** `import { TcpListener, TcpStream, UdpSocket } from "std:net"`

```typescript
// TCP server
const listener = new TcpListener("127.0.0.1", 8080);
for (const stream of listener.accept()) {
  const data = stream.read();
  stream.write(data);  // Echo
}

// TCP client
const client = TcpStream.connect("example.com", 80);
client.write("GET / HTTP/1.0\r\n\r\n");
const response = client.read();

// UDP
const socket = new UdpSocket("0.0.0.0", 9000);
const [data, addr] = socket.recvFrom();
socket.sendTo(data, addr);
```

## http (HTTP Server)

**Import:** `import { HttpServer } from "std:http"`

```typescript
const server = new HttpServer("127.0.0.1", 8080);

server.serve((req) => {
  if (req.path == "/") {
    return {
      status: 200,
      headers: { "Content-Type": "text/html" },
      body: "<h1>Hello!</h1>"
    };
  }
  return { status: 404, body: "Not Found" };
});
```

## fetch (HTTP Client)

**Import:** `import fetch from "std:fetch"`

```typescript
const response = fetch("https://api.example.com/data");
logger.info(response.status, response.body);

// POST request
const result = fetch("https://api.example.com/submit", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ key: "value" })
});
```

## env (Environment)

**Import:** `import env from "std:env"`

```typescript
const home = env.get("HOME");
env.set("MY_VAR", "value");
const vars = env.all();  // All environment variables
```

## process (Process Management)

**Import:** `import process from "std:process"`

```typescript
// Execute command
const result = process.exec("ls", ["-la"]);
logger.info(result.stdout);

// Exit
process.exit(0);

// Process info
logger.info(process.pid());
logger.info(process.argv());
```

## os (Platform Info)

**Import:** `import os from "std:os"`

```typescript
logger.info(os.platform());    // "linux", "macos", "windows"
logger.info(os.arch());        // "x86_64", "aarch64"
logger.info(os.cpus());        // Number of CPUs
logger.info(os.totalMemory()); // Total RAM in bytes
logger.info(os.freeMemory());  // Available RAM
```

## io (Standard I/O)

**Import:** `import io from "std:io"`

```typescript
io.print("Enter name: ");
const name = io.readLine();
logger.info("Hello,", name);

io.printErr("Error occurred");
```

## dns (DNS Resolution)

**Import:** `import dns from "std:dns"`

```typescript
const addrs = dns.lookup("example.com");
for (const addr of addrs) {
  logger.info(addr);
}
```

## terminal (Terminal Control)

**Import:** `import terminal from "std:terminal"`

```typescript
terminal.clear();
terminal.moveTo(10, 5);
terminal.setColor("red");
terminal.write("Error!");
terminal.reset();
```

## ws (WebSocket Client)

**Import:** `import ws from "std:ws"`

```typescript
const socket = ws.connect("wss://echo.websocket.org");
socket.send("Hello, WebSocket!");
const msg = socket.receive();
logger.info(msg);
socket.close();
```

## readline (Line Editing)

**Import:** `import readline from "std:readline"`

```typescript
const rl = new readline.Readline();
rl.addHistory("previous command");

const input = rl.readLine("prompt> ");
logger.info("You entered:", input);
```

## glob (File Globbing)

**Import:** `import glob from "std:glob"`

```typescript
const files = glob.match("**/*.raya");
for (const file of files) {
  logger.info(file);
}
```

## archive (tar/zip)

**Import:** `import archive from "std:archive"`

```typescript
// Create tar
archive.createTar("output.tar", ["file1.txt", "file2.txt"]);

// Extract tar
archive.extractTar("archive.tar", "./output/");

// Create zip
archive.createZip("output.zip", ["file1.txt", "file2.txt"]);

// Extract zip
archive.extractZip("archive.zip", "./output/");
```

## watch (File Watching)

**Import:** `import watch from "std:watch"`

```typescript
const watcher = watch.create("./src");

for (const event of watcher.poll()) {
  logger.info(event.type, event.path);  // "created", "modified", "deleted"
}
```

---

**Platform Support:** Linux, macOS, BSD, WSL
