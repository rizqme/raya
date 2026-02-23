//! End-to-end tests for the std:net module
//!
//! Covers TCP listener/stream round-trips and UDP send/receive helpers.

use super::harness::expect_bool_with_builtins;

#[test]
fn test_net_tcp_echo_round_trip() {
    expect_bool_with_builtins(
        r#"
        import net, { TcpListener } from "std:net";

        async function serverTask(listener: TcpListener): Task<boolean> {
            const stream = listener.accept();
            if (stream == null) {
                listener.close();
                return false;
            }
            const message = stream.readLine();
            stream.writeText("echo:" + message + "\n");
            stream.close();
            listener.close();
            return message == "ping";
        }

        async function clientTask(host: string, port: number): Task<boolean> {
            const stream = net.connect(host, port);
            const remote = stream.remoteAddr();
            stream.writeText("ping\n");
            const response = stream.readLine();
            stream.close();
            return response == "echo:ping" && remote.length > 0;
        }

        async function main(): Task<boolean> {
            const port = 38191;
            const listener = net.listen("127.0.0.1", port);
            const serverResult = serverTask(listener);
            sleep(5);
            const clientOk = await clientTask("127.0.0.1", port);
            if (!clientOk) {
                // Ensure accept() is unblocked on failure paths so test never hangs.
                listener.close();
            }
            const serverOk = await serverResult;
            return serverOk && clientOk;
        }

        return await main();
        "#,
        true,
    );
}

#[test]
fn test_net_accept_returns_null_after_close() {
    expect_bool_with_builtins(
        r#"
        import net, { TcpListener } from "std:net";

        async function acceptLoop(listener: TcpListener): Task<number> {
            let accepted: number = 0;
            while (true) {
                const stream = listener.accept();
                if (stream == null) {
                    break;
                }
                accepted = accepted + 1;
                stream.close();
            }
            return accepted;
        }

        async function main(): Task<boolean> {
            const listener = net.listen("127.0.0.1", 0);
            const loopTask = acceptLoop(listener);
            sleep(10);
            listener.close();
            const accepted = await loopTask;
            return accepted == 0;
        }

        return await main();
        "#,
        true,
    );
}

#[test]
fn test_net_serve_exits_when_listener_closed() {
    expect_bool_with_builtins(
        r#"
        import net, { TcpStream } from "std:net";

        async function onConn(stream: TcpStream): Task<void> {
            stream.close();
        }

        async function main(): Task<boolean> {
            const listener = net.listen("127.0.0.1", 0);
            const serveTask = listener.serve(onConn);
            sleep(10);
            listener.close();
            await serveTask;
            return true;
        }

        return await main();
        "#,
        true,
    );
}

#[test]
fn test_net_udp_send_receive() {
    expect_bool_with_builtins(
        r#"
        import net from "std:net";

        function main(): boolean {
            const receiver = net.bindUdp("localhost", 0);
            const sender = net.bindUdp("localhost", 0);
            sender.sendText("udp_ping", receiver.localAddr());
            const received = receiver.receive(64);
            receiver.close();
            sender.close();
            return received.toUtf8String() == "udp_ping";
        }

        return main();
        "#,
        true,
    );
}
