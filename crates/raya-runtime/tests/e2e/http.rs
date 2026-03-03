//! End-to-end tests for the std:http module
//!
//! Verifies HTTP server request parsing, response body delivery, and custom headers.

use super::harness::expect_bool_with_builtins;

#[test]
fn test_http_server_parses_and_responds() {
    expect_bool_with_builtins(
        r#"
        import net from "std:net";
        import { HttpServer, HttpRequest } from "std:http";

        async function onRequest(req: HttpRequest, server: HttpServer): Promise<void> {
            server.respondText(req, 200, "ok-from-serve");
        }

        async function serverTask(server: HttpServer): Promise<boolean> {
            let req: HttpRequest;
            try {
                req = await async server.accept();
            } catch (_err) {
                return false;
            }
            const methodOk = req.method() == "POST";
            const pathOk = req.path() == "/test";
            const queryOk = req.query() == "id=7";
            const headerOk = req.header("custom") == "value";
            const bodyOk = req.body() == "payload";
            server.respond(req._handle, 200, "server-ok");
            server.close();
            return methodOk && pathOk && queryOk && headerOk && bodyOk;
        }

        async function clientTask(): Promise<boolean> {
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

        async function main(): Promise<boolean> {
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
        import { HttpServer, HttpRequest } from "std:http";

        async function onRequest(req: HttpRequest, server: HttpServer): Promise<void> {
            server.respondText(req, 200, "ok-from-serve");
        }

        async function serverTask(server: HttpServer): Promise<boolean> {
            let req: HttpRequest;
            try {
                req = await async server.accept();
            } catch (_err) {
                return false;
            }
            const headerValue = req.header("x-test");
            server.respondWithHeaders(req._handle, 201, ["X-Test", "header-value"], "payload");
            server.close();
            return headerValue == "header-value";
        }

        async function clientTask(): Promise<boolean> {
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

        async function main(): Promise<boolean> {
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

#[test]
fn test_http_server_serve_task_cancel() {
    expect_bool_with_builtins(
        r#"
        import net from "std:net";
        import { HttpServer, HttpRequest } from "std:http";

        async function onRequest(req: HttpRequest, server: HttpServer): Promise<void> {
            server.respondText(req, 200, "ok-from-serve");
        }

        async function clientTask(port: number): Promise<boolean> {
            const stream = net.connect("127.0.0.1", port);
            const request =
                "GET /serve HTTP/1.1\r\n" +
                "Host: 127.0.0.1\r\n" +
                "\r\n";
            stream.writeText(request);
            const response = stream.readAll().toUtf8String();
            stream.close();
            return response.includes("HTTP/1.1 200 OK") && response.includes("ok-from-serve");
        }

        async function main(): Promise<boolean> {
            const server = new HttpServer("127.0.0.1", 0);
            const port = server.localPort();
            const serveTask = server.serve(onRequest);
            sleep(5);
            const ok = await clientTask(port);
            server.close();
            serveTask.cancel();
            return ok;
        }

        return await main();
        "#,
        true,
    );
}
