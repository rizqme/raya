//! End-to-end tests for the std:http module
//!
//! Verifies HTTP server request parsing, response body delivery, and custom headers.

use super::harness::expect_bool_with_builtins;

#[test]
fn test_http_server_parses_and_responds() {
    expect_bool_with_builtins(
        r#"
        import net from "std:net";
        import { HttpServer } from "std:http";

        async function serverTask(server: HttpServer): Task<boolean> {
            const req = server.accept();
            const methodOk = req.method() == "POST";
            const pathOk = req.path() == "/test";
            const queryOk = req.query() == "id=7";
            const headerOk = req.header("custom") == "value";
            const bodyOk = req.body() == "payload";
            server.respond(req._handle, 200, "server-ok");
            server.close();
            return methodOk && pathOk && queryOk && headerOk && bodyOk;
        }

        async function clientTask(): Task<boolean> {
            const stream = net.connect("127.0.0.1", 38181);
            const request =
                "POST /test?id=7 HTTP/1.1\r\n" +
                "Host: 127.0.0.1\r\n" +
                "Custom: value\r\n" +
                "Content-Length: 7\r\n" +
                "\r\n" +
                "payload";
            stream.writeText(request);
            const response = stream.readAll().toUtf8String();
            stream.close();
            return response.includes("HTTP/1.1 200 OK") && response.includes("server-ok");
        }

        async function main(): Task<boolean> {
            const server = new HttpServer("127.0.0.1", 38181);
            const serverResult = serverTask(server);
            sleep(5);
            const clientOk = await clientTask();
            const serverOk = await serverResult;
            return clientOk && serverOk;
        }

        return await main();
        "#,
        true,
    );
}

#[test]
fn test_http_server_custom_headers() {
    expect_bool_with_builtins(
        r#"
        import net from "std:net";
        import { HttpServer } from "std:http";

        async function serverTask(server: HttpServer): Task<boolean> {
            const req = server.accept();
            const headerValue = req.header("x-test");
            server.respondWithHeaders(req._handle, 201, ["X-Test", "header-value"], "payload");
            server.close();
            return headerValue == "header-value";
        }

        async function clientTask(): Task<boolean> {
            const stream = net.connect("127.0.0.1", 38182);
            const request =
                "GET /headers HTTP/1.1\r\n" +
                "Host: 127.0.0.1\r\n" +
                "x-test: header-value\r\n" +
                "\r\n";
            stream.writeText(request);
            const response = stream.readAll().toUtf8String();
            stream.close();
            return response.includes("HTTP/1.1 201") && response.includes("X-Test: header-value");
        }

        async function main(): Task<boolean> {
            const server = new HttpServer("127.0.0.1", 38182);
            const serverResult = serverTask(server);
            sleep(5);
            const clientOk = await clientTask();
            const serverOk = await serverResult;
            return clientOk && serverOk;
        }

        return await main();
        "#,
        true,
    );
}
