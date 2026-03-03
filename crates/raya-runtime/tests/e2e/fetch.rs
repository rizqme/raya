//! End-to-end tests for the std:fetch module
//!
//! Uses a local std:http server to validate fetch responses, headers, and body accessors.

use super::harness::expect_bool_with_builtins;

#[test]
fn test_fetch_get_request_and_response() {
    expect_bool_with_builtins(
        r#"
        import fetch from "std:fetch";
        import { HttpServer } from "std:http";

        async function serverTask(server: HttpServer): Promise<boolean> {
            let req: HttpRequest;
            try {
                req = await async server.accept();
            } catch (_err) {
                return false;
            }
            const methodOk = req.method() == "GET";
            const pathOk = req.path() == "/fetch";
            const queryOk = req.query() == "q=1";
            server.respond(req._handle, 200, "{\"status\":\"ok\"}");
            server.close();
            return methodOk && pathOk && queryOk;
        }

        async function main(): Promise<boolean> {
            const server = new HttpServer("127.0.0.1", 38185);
            const serverResult = serverTask(server);
            sleep(5);
            const response = fetch.get("http://127.0.0.1:38185/fetch?q=1");
            const statusOk = response.status() == 200 && response.ok();
            const body = response.text();
            response.release();
            const serverOk = await serverResult;
            return statusOk && body == "{\"status\":\"ok\"}" && serverOk;
        }

        return await main();
        "#,
        true,
    );
}

#[test]
fn test_fetch_post_with_extra_headers() {
    expect_bool_with_builtins(
        r#"
        import fetch from "std:fetch";
        import { HttpServer } from "std:http";

        async function serverTask(server: HttpServer): Promise<boolean> {
            let req: HttpRequest;
            try {
                req = await async server.accept();
            } catch (_err) {
                return false;
            }
            const methodOk = req.method() == "POST";
            const headerOk = req.header("x-api") == "true";
            const bodyOk = req.body() == "payload";
            server.respond(req._handle, 201, "accepted");
            server.close();
            return methodOk && headerOk && bodyOk;
        }

        async function main(): Promise<boolean> {
            const server = new HttpServer("127.0.0.1", 38186);
            const serverResult = serverTask(server);
            sleep(5);
            const response = fetch.request(
                "POST",
                "http://127.0.0.1:38186/submit",
                "payload",
                "X-Api: true"
            );
            const statusOk = response.status() == 201 && response.statusText() == "Created";
            const body = response.text();
            response.release();
            const serverOk = await serverResult;
            return statusOk && body == "accepted" && serverOk;
        }

        return await main();
        "#,
        true,
    );
}
