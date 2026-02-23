//! End-to-end tests for the std:net module
//!
//! Covers TCP listener/stream round-trips and UDP send/receive helpers.

use super::harness::expect_bool_with_builtins;

#[test]
fn test_net_tcp_echo_round_trip() {
    expect_bool_with_builtins(
        r#"
        import net, { TcpListener } from "std:net";

        function parseDigits(text: string): number {
            let value: number = 0;
            let idx: number = 0;
            while (idx < text.length) {
                value = value * 10 + (text.charCodeAt(idx) - 48);
                idx = idx + 1;
            }
            return value;
        }

        function parsePortFromAddr(addr: string): number {
            const parts: string[] = addr.split(":");
            return parseDigits(parts[parts.length - 1]);
        }

        async function serverTask(listener: TcpListener): Task<boolean> {
            const stream = listener.accept();
            const message = stream.readLine();
            stream.writeText("echo:" + message + "\n");
            stream.close();
            listener.close();
            return message == "ping";
        }

        async function clientTask(port: number): Task<boolean> {
            const stream = net.connect("127.0.0.1", port);
            const remote = stream.remoteAddr();
            stream.writeText("ping\n");
            const response = stream.readLine();
            stream.close();
            return response == "echo:ping" && remote.length > 0;
        }

        async function main(): Task<boolean> {
            const listener = net.listen("127.0.0.1", 0);
            const port = parsePortFromAddr(listener.localAddr());
            const results = await [serverTask(listener), clientTask(port)];
            return results[0] && results[1];
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

        function parseDigits(text: string): number {
            let value: number = 0;
            let idx: number = 0;
            while (idx < text.length) {
                value = value * 10 + (text.charCodeAt(idx) - 48);
                idx = idx + 1;
            }
            return value;
        }

        function parsePortFromAddr(addr: string): number {
            const parts: string[] = addr.split(":");
            return parseDigits(parts[parts.length - 1]);
        }

        function main(): boolean {
            const receiver = net.bindUdp("127.0.0.1", 0);
            const sender = net.bindUdp("127.0.0.1", 0);
            const port = parsePortFromAddr(receiver.localAddr());
            sender.sendText("udp_ping", "127.0.0.1:" + port);
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
