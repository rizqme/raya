//! End-to-end tests for the std:net module
//!
//! Covers TCP listener/stream round-trips and UDP send/receive helpers.

use super::harness::expect_bool_with_builtins;

#[test]
fn test_net_tcp_echo_round_trip() {
    expect_bool_with_builtins(
        r#"
        import net, { TcpListener } from "std:net";
        import time from "std:time";

        function bindListener(): TcpListener {
            let attempts = 0;
            while (attempts < 32) {
                const candidate = 30000 + ((time.now() + attempts * 7919) % 20000);
                try {
                    return net.listen("127.0.0.1", candidate);
                } catch (_err) {
                    attempts = attempts + 1;
                }
            }
            return net.listen("127.0.0.1", 0);
        }

        async function main(): Promise<boolean> {
            const listener = bindListener();
            const addr = listener.localAddr();
            const sep = addr.lastIndexOf(":");
            const port: number = JSON.parse(addr.slice(sep + 1));
            const acceptTask = async listener.accept();
            sleep(5);
            const client = net.connect("127.0.0.1", port);
            const remote = client.remoteAddr();
            client.writeText("ping\n");
            const serverStream = await acceptTask;
            if (serverStream == null) {
                client.close();
                listener.close();
                return false;
            }
            const message = serverStream.readLine();
            serverStream.writeText("echo:" + message + "\n");
            serverStream.close();
            listener.close();
            const response = client.readLine();
            client.close();
            const clientOk = response == "echo:ping" && remote.length > 0;
            const serverOk = message == "ping";
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

        async function main(): Promise<boolean> {
            const listener = net.listen("127.0.0.1", 0);
            const acceptTask = async listener.accept();
            sleep(10);
            listener.close();
            const stream = await acceptTask;
            return stream == null;
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

        async function onConn(stream: TcpStream): Promise<void> {
            stream.close();
        }

        async function main(): Promise<boolean> {
            const listener = net.listen("127.0.0.1", 0);
            const serveTask = async listener.serve(onConn);
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
