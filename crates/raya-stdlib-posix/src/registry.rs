//! Symbolic native function registry for POSIX stdlib
//!
//! Registers all POSIX stdlib native functions by symbolic name
//! (e.g., "fs.readFile", "net.tcpListen") into a `NativeFunctionRegistry`.

use raya_sdk::NativeFunctionRegistry;

/// Register all POSIX stdlib native functions into the given registry.
pub fn register_posix(registry: &mut NativeFunctionRegistry) {
    register_env(registry);
    register_os(registry);
    register_io(registry);
    register_fs(registry);
    register_process(registry);
    register_net(registry);
    register_http(registry);
    register_fetch(registry);
}

fn register_env(registry: &mut NativeFunctionRegistry) {
    registry.register("env.get", |ctx, args| crate::env::get(ctx, args));
    registry.register("env.set", |ctx, args| crate::env::set(ctx, args));
    registry.register("env.remove", |ctx, args| crate::env::remove(ctx, args));
    registry.register("env.has", |ctx, args| crate::env::has(ctx, args));
    registry.register("env.all", |ctx, args| crate::env::all(ctx, args));
    registry.register("env.cwd", |ctx, args| crate::env::cwd(ctx, args));
    registry.register("env.home", |ctx, args| crate::env::home(ctx, args));
}

fn register_os(registry: &mut NativeFunctionRegistry) {
    registry.register("os.platform", |ctx, args| crate::os::platform(ctx, args));
    registry.register("os.arch", |ctx, args| crate::os::arch(ctx, args));
    registry.register("os.hostname", |ctx, args| crate::os::hostname(ctx, args));
    registry.register("os.cpus", |ctx, args| crate::os::cpus(ctx, args));
    registry.register("os.totalMemory", |ctx, args| crate::os::total_memory(ctx, args));
    registry.register("os.freeMemory", |ctx, args| crate::os::free_memory(ctx, args));
    registry.register("os.uptime", |ctx, args| crate::os::uptime(ctx, args));
    registry.register("os.eol", |ctx, args| crate::os::eol(ctx, args));
    registry.register("os.tmpdir", |ctx, args| crate::os::tmpdir(ctx, args));
}

fn register_io(registry: &mut NativeFunctionRegistry) {
    registry.register("io.readLine", |ctx, args| crate::io::read_line(ctx, args));
    registry.register("io.readAll", |ctx, args| crate::io::read_all(ctx, args));
    registry.register("io.write", |ctx, args| crate::io::write(ctx, args));
    registry.register("io.writeln", |ctx, args| crate::io::writeln(ctx, args));
    registry.register("io.writeErr", |ctx, args| crate::io::write_err(ctx, args));
    registry.register("io.writeErrln", |ctx, args| crate::io::write_errln(ctx, args));
    registry.register("io.flush", |ctx, args| crate::io::flush(ctx, args));
}

fn register_fs(registry: &mut NativeFunctionRegistry) {
    registry.register("fs.readFile", |ctx, args| crate::fs::read_file(ctx, args));
    registry.register("fs.readTextFile", |ctx, args| crate::fs::read_text_file(ctx, args));
    registry.register("fs.writeFile", |ctx, args| crate::fs::write_file(ctx, args));
    registry.register("fs.writeTextFile", |ctx, args| crate::fs::write_text_file(ctx, args));
    registry.register("fs.appendFile", |ctx, args| crate::fs::append_file(ctx, args));
    registry.register("fs.exists", |ctx, args| crate::fs::exists(ctx, args));
    registry.register("fs.isFile", |ctx, args| crate::fs::is_file(ctx, args));
    registry.register("fs.isDir", |ctx, args| crate::fs::is_dir(ctx, args));
    registry.register("fs.isSymlink", |ctx, args| crate::fs::is_symlink(ctx, args));
    registry.register("fs.fileSize", |ctx, args| crate::fs::file_size(ctx, args));
    registry.register("fs.lastModified", |ctx, args| crate::fs::last_modified(ctx, args));
    registry.register("fs.stat", |ctx, args| crate::fs::stat(ctx, args));
    registry.register("fs.mkdir", |ctx, args| crate::fs::mkdir(ctx, args));
    registry.register("fs.mkdirRecursive", |ctx, args| crate::fs::mkdir_recursive(ctx, args));
    registry.register("fs.readDir", |ctx, args| crate::fs::read_dir(ctx, args));
    registry.register("fs.rmdir", |ctx, args| crate::fs::rmdir(ctx, args));
    registry.register("fs.remove", |ctx, args| crate::fs::remove(ctx, args));
    registry.register("fs.rename", |ctx, args| crate::fs::rename(ctx, args));
    registry.register("fs.copy", |ctx, args| crate::fs::copy(ctx, args));
    registry.register("fs.chmod", |ctx, args| crate::fs::chmod(ctx, args));
    registry.register("fs.symlink", |ctx, args| crate::fs::symlink(ctx, args));
    registry.register("fs.readlink", |ctx, args| crate::fs::readlink(ctx, args));
    registry.register("fs.realpath", |ctx, args| crate::fs::realpath(ctx, args));
    registry.register("fs.tempDir", |ctx, args| crate::fs::temp_dir(ctx, args));
    registry.register("fs.tempFile", |ctx, args| crate::fs::temp_file(ctx, args));
}

fn register_process(registry: &mut NativeFunctionRegistry) {
    registry.register("process.exit", |ctx, args| crate::process::exit(ctx, args));
    registry.register("process.pid", |ctx, args| crate::process::pid(ctx, args));
    registry.register("process.argv", |ctx, args| crate::process::argv(ctx, args));
    registry.register("process.execPath", |ctx, args| crate::process::exec_path(ctx, args));
    registry.register("process.exec", |ctx, args| crate::process::exec(ctx, args));
    registry.register("process.execGetCode", |ctx, args| crate::process::exec_get_code(ctx, args));
    registry.register("process.execGetStdout", |ctx, args| crate::process::exec_get_stdout(ctx, args));
    registry.register("process.execGetStderr", |ctx, args| crate::process::exec_get_stderr(ctx, args));
    registry.register("process.execRelease", |ctx, args| crate::process::exec_release(ctx, args));
}

fn register_net(registry: &mut NativeFunctionRegistry) {
    registry.register("net.tcpListen", |ctx, args| crate::net::tcp_listen(ctx, args));
    registry.register("net.tcpAccept", |ctx, args| crate::net::tcp_accept(ctx, args));
    registry.register("net.tcpListenerClose", |ctx, args| crate::net::tcp_listener_close(ctx, args));
    registry.register("net.tcpListenerAddr", |ctx, args| crate::net::tcp_listener_addr(ctx, args));
    registry.register("net.tcpConnect", |ctx, args| crate::net::tcp_connect(ctx, args));
    registry.register("net.tcpRead", |ctx, args| crate::net::tcp_read(ctx, args));
    registry.register("net.tcpReadAll", |ctx, args| crate::net::tcp_read_all(ctx, args));
    registry.register("net.tcpReadLine", |ctx, args| crate::net::tcp_read_line(ctx, args));
    registry.register("net.tcpWrite", |ctx, args| crate::net::tcp_write(ctx, args));
    registry.register("net.tcpWriteText", |ctx, args| crate::net::tcp_write_text(ctx, args));
    registry.register("net.tcpStreamClose", |ctx, args| crate::net::tcp_stream_close(ctx, args));
    registry.register("net.tcpRemoteAddr", |ctx, args| crate::net::tcp_remote_addr(ctx, args));
    registry.register("net.tcpLocalAddr", |ctx, args| crate::net::tcp_local_addr(ctx, args));
    registry.register("net.udpBind", |ctx, args| crate::net::udp_bind(ctx, args));
    registry.register("net.udpSendTo", |ctx, args| crate::net::udp_send_to(ctx, args));
    registry.register("net.udpSendText", |ctx, args| crate::net::udp_send_text(ctx, args));
    registry.register("net.udpReceive", |ctx, args| crate::net::udp_receive(ctx, args));
    registry.register("net.udpClose", |ctx, args| crate::net::udp_close(ctx, args));
    registry.register("net.udpLocalAddr", |ctx, args| crate::net::udp_local_addr(ctx, args));
}

fn register_http(registry: &mut NativeFunctionRegistry) {
    registry.register("http.serverCreate", |ctx, args| crate::http::server_create(ctx, args));
    registry.register("http.serverAccept", |ctx, args| crate::http::server_accept(ctx, args));
    registry.register("http.serverRespond", |ctx, args| crate::http::server_respond(ctx, args));
    registry.register("http.serverRespondBytes", |ctx, args| crate::http::server_respond_bytes(ctx, args));
    registry.register("http.serverRespondHeaders", |ctx, args| crate::http::server_respond_headers(ctx, args));
    registry.register("http.serverClose", |ctx, args| crate::http::server_close(ctx, args));
    registry.register("http.serverAddr", |ctx, args| crate::http::server_addr(ctx, args));
    registry.register("http.reqMethod", |ctx, args| crate::http::req_method(ctx, args));
    registry.register("http.reqPath", |ctx, args| crate::http::req_path(ctx, args));
    registry.register("http.reqQuery", |ctx, args| crate::http::req_query(ctx, args));
    registry.register("http.reqHeader", |ctx, args| crate::http::req_header(ctx, args));
    registry.register("http.reqHeaders", |ctx, args| crate::http::req_headers(ctx, args));
    registry.register("http.reqBody", |ctx, args| crate::http::req_body(ctx, args));
    registry.register("http.reqBodyBytes", |ctx, args| crate::http::req_body_bytes(ctx, args));
}

fn register_fetch(registry: &mut NativeFunctionRegistry) {
    registry.register("fetch.request", |ctx, args| crate::fetch::request(ctx, args));
    registry.register("fetch.resStatus", |ctx, args| crate::fetch::res_status(ctx, args));
    registry.register("fetch.resStatusText", |ctx, args| crate::fetch::res_status_text(ctx, args));
    registry.register("fetch.resHeader", |ctx, args| crate::fetch::res_header(ctx, args));
    registry.register("fetch.resHeaders", |ctx, args| crate::fetch::res_headers(ctx, args));
    registry.register("fetch.resText", |ctx, args| crate::fetch::res_text(ctx, args));
    registry.register("fetch.resBytes", |ctx, args| crate::fetch::res_bytes(ctx, args));
}
