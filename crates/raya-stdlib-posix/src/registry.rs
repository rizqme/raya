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
    register_dns(registry);
    register_terminal(registry);
    register_ws(registry);
    register_readline(registry);
    register_glob(registry);
    register_archive(registry);
    register_watch(registry);
}

fn register_env(registry: &mut NativeFunctionRegistry) {
    registry.register("env.get", |ctx, args| crate::env::get(ctx, args));
    registry.register("env.set", |ctx, args| crate::env::set(ctx, args));
    registry.register("env.remove", |ctx, args| crate::env::remove(ctx, args));
    registry.register("env.has", |ctx, args| crate::env::has(ctx, args));
    registry.register("env.all", |ctx, args| crate::env::all(ctx, args));
    registry.register("env.cwd", |ctx, args| crate::env::cwd(ctx, args));
    registry.register("env.home", |ctx, args| crate::env::home(ctx, args));
    registry.register("env.configDir", |ctx, args| crate::env::config_dir(ctx, args));
    registry.register("env.cacheDir", |ctx, args| crate::env::cache_dir(ctx, args));
    registry.register("env.dataDir", |ctx, args| crate::env::data_dir(ctx, args));
    registry.register("env.stateDir", |ctx, args| crate::env::state_dir(ctx, args));
    registry.register("env.runtimeDir", |ctx, args| crate::env::runtime_dir(ctx, args));
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
    registry.register("os.release", |ctx, args| crate::os::release(ctx, args));
    registry.register("os.osType", |ctx, args| crate::os::os_type(ctx, args));
    registry.register("os.machine", |ctx, args| crate::os::machine(ctx, args));
    registry.register("os.username", |ctx, args| crate::os::username(ctx, args));
    registry.register("os.userInfo", |ctx, args| crate::os::user_info(ctx, args));
    registry.register("os.shell", |ctx, args| crate::os::shell(ctx, args));
    registry.register("os.loadavg", |ctx, args| crate::os::loadavg(ctx, args));
    registry.register("os.networkInterfaces", |ctx, args| crate::os::network_interfaces(ctx, args));
    registry.register("os.endianness", |ctx, args| crate::os::endianness(ctx, args));
    registry.register("os.pageSize", |ctx, args| crate::os::page_size(ctx, args));
}

fn register_io(registry: &mut NativeFunctionRegistry) {
    registry.register("io.readLine", |ctx, args| crate::io::read_line(ctx, args));
    registry.register("io.readAll", |ctx, args| crate::io::read_all(ctx, args));
    registry.register("io.readExact", |ctx, args| crate::io::read_exact(ctx, args));
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
    registry.register("process.ppid", |ctx, args| crate::process::ppid(ctx, args));
    registry.register("process.version", |ctx, args| crate::process::version(ctx, args));
    registry.register("process.uptime", |ctx, args| crate::process::uptime(ctx, args));
    registry.register("process.memoryUsage", |ctx, args| crate::process::memory_usage(ctx, args));
    registry.register("process.cpuUsage", |ctx, args| crate::process::cpu_usage(ctx, args));
    registry.register("process.chdir", |ctx, args| crate::process::chdir(ctx, args));
    registry.register("process.umaskGet", |ctx, args| crate::process::umask_get(ctx, args));
    registry.register("process.umaskSet", |ctx, args| crate::process::umask_set(ctx, args));
    registry.register("process.uid", |ctx, args| crate::process::uid(ctx, args));
    registry.register("process.gid", |ctx, args| crate::process::gid(ctx, args));
    registry.register("process.euid", |ctx, args| crate::process::euid(ctx, args));
    registry.register("process.egid", |ctx, args| crate::process::egid(ctx, args));
    registry.register("process.groups", |ctx, args| crate::process::groups(ctx, args));
    // Process title and heap
    registry.register("process.title", |ctx, args| crate::process::title(ctx, args));
    registry.register("process.setTitle", |ctx, args| crate::process::set_title(ctx, args));
    registry.register("process.heapUsed", |ctx, args| crate::process::heap_used(ctx, args));
    registry.register("process.heapTotal", |ctx, args| crate::process::heap_total(ctx, args));
    // Signal handling
    registry.register("process.trapSignal", |ctx, args| crate::process::trap_signal(ctx, args));
    registry.register("process.untrapSignal", |ctx, args| crate::process::untrap_signal(ctx, args));
    registry.register("process.hasSignal", |ctx, args| crate::process::has_signal(ctx, args));
    registry.register("process.clearSignal", |ctx, args| crate::process::clear_signal(ctx, args));
    registry.register("process.waitSignal", |ctx, args| crate::process::wait_signal(ctx, args));
    // Child process (subprocess spawning)
    registry.register("process.spawn", |ctx, args| crate::process::process_spawn(ctx, args));
    registry.register("process.spawnWithArgs", |ctx, args| crate::process::process_spawn_with_args(ctx, args));
    registry.register("process.spawnWithOptions", |ctx, args| crate::process::process_spawn_with_options(ctx, args));
    registry.register("process.childWait", |ctx, args| crate::process::child_wait(ctx, args));
    registry.register("process.childTryWait", |ctx, args| crate::process::child_try_wait(ctx, args));
    registry.register("process.childIsAlive", |ctx, args| crate::process::child_is_alive(ctx, args));
    registry.register("process.childWriteStdin", |ctx, args| crate::process::child_write_stdin(ctx, args));
    registry.register("process.childReadStdout", |ctx, args| crate::process::child_read_stdout(ctx, args));
    registry.register("process.childReadStderr", |ctx, args| crate::process::child_read_stderr(ctx, args));
    registry.register("process.childKill", |ctx, args| crate::process::child_kill(ctx, args));
    registry.register("process.childSignal", |ctx, args| crate::process::child_signal(ctx, args));
    registry.register("process.childPid", |ctx, args| crate::process::child_pid(ctx, args));
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
    // TLS streams
    registry.register("net.tlsConnect", |ctx, args| crate::net::tls_connect(ctx, args));
    registry.register("net.tlsConnectWithCa", |ctx, args| crate::net::tls_connect_with_ca(ctx, args));
    registry.register("net.tlsRead", |ctx, args| crate::net::tls_read(ctx, args));
    registry.register("net.tlsReadAll", |ctx, args| crate::net::tls_read_all(ctx, args));
    registry.register("net.tlsReadLine", |ctx, args| crate::net::tls_read_line(ctx, args));
    registry.register("net.tlsWrite", |ctx, args| crate::net::tls_write(ctx, args));
    registry.register("net.tlsWriteText", |ctx, args| crate::net::tls_write_text(ctx, args));
    registry.register("net.tlsClose", |ctx, args| crate::net::tls_close(ctx, args));
    registry.register("net.tlsRemoteAddr", |ctx, args| crate::net::tls_remote_addr(ctx, args));
    registry.register("net.tlsLocalAddr", |ctx, args| crate::net::tls_local_addr(ctx, args));
}

fn register_http(registry: &mut NativeFunctionRegistry) {
    registry.register("http.serverCreate", |ctx, args| crate::http::server_create(ctx, args));
    registry.register("http.serverCreateTls", |ctx, args| crate::http::server_create_tls(ctx, args));
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
    registry.register("http.reqUrl", |ctx, args| crate::http::req_url(ctx, args));
    registry.register("http.reqRemoteAddr", |ctx, args| crate::http::req_remote_addr(ctx, args));
}

fn register_fetch(registry: &mut NativeFunctionRegistry) {
    registry.register("fetch.request", |ctx, args| crate::fetch::request(ctx, args));
    registry.register("fetch.resStatus", |ctx, args| crate::fetch::res_status(ctx, args));
    registry.register("fetch.resStatusText", |ctx, args| crate::fetch::res_status_text(ctx, args));
    registry.register("fetch.resHeader", |ctx, args| crate::fetch::res_header(ctx, args));
    registry.register("fetch.resHeaders", |ctx, args| crate::fetch::res_headers(ctx, args));
    registry.register("fetch.resText", |ctx, args| crate::fetch::res_text(ctx, args));
    registry.register("fetch.resBytes", |ctx, args| crate::fetch::res_bytes(ctx, args));
    registry.register("fetch.resRelease", |ctx, args| crate::fetch::res_release(ctx, args));
    registry.register("fetch.resOk", |ctx, args| crate::fetch::res_ok(ctx, args));
    registry.register("fetch.resRedirected", |ctx, args| crate::fetch::res_redirected(ctx, args));
}

fn register_dns(registry: &mut NativeFunctionRegistry) {
    registry.register("dns.lookup", |ctx, args| crate::dns::dns_lookup(ctx, args));
    registry.register("dns.lookup4", |ctx, args| crate::dns::dns_lookup4(ctx, args));
    registry.register("dns.lookup6", |ctx, args| crate::dns::dns_lookup6(ctx, args));
    registry.register("dns.lookupMx", |ctx, args| crate::dns::dns_lookup_mx(ctx, args));
    registry.register("dns.lookupTxt", |ctx, args| crate::dns::dns_lookup_txt(ctx, args));
    registry.register("dns.lookupSrv", |ctx, args| crate::dns::dns_lookup_srv(ctx, args));
    registry.register("dns.lookupCname", |ctx, args| crate::dns::dns_lookup_cname(ctx, args));
    registry.register("dns.lookupNs", |ctx, args| crate::dns::dns_lookup_ns(ctx, args));
    registry.register("dns.reverse", |ctx, args| crate::dns::dns_reverse(ctx, args));
}

fn register_terminal(registry: &mut NativeFunctionRegistry) {
    // TTY detection
    registry.register("terminal.isTerminal", |ctx, args| crate::terminal::is_terminal(ctx, args));
    registry.register("terminal.isTerminalStdin", |ctx, args| crate::terminal::is_terminal_stdin(ctx, args));
    registry.register("terminal.isTerminalStderr", |ctx, args| crate::terminal::is_terminal_stderr(ctx, args));
    // Terminal size
    registry.register("terminal.columns", |ctx, args| crate::terminal::columns(ctx, args));
    registry.register("terminal.rows", |ctx, args| crate::terminal::rows(ctx, args));
    // Raw mode
    registry.register("terminal.enableRawMode", |ctx, args| crate::terminal::enable_raw_mode(ctx, args));
    registry.register("terminal.disableRawMode", |ctx, args| crate::terminal::disable_raw_mode(ctx, args));
    // Input
    registry.register("terminal.readKey", |ctx, args| crate::terminal::read_key(ctx, args));
    // Cursor control
    registry.register("terminal.moveTo", |ctx, args| crate::terminal::move_to(ctx, args));
    registry.register("terminal.moveUp", |ctx, args| crate::terminal::move_up(ctx, args));
    registry.register("terminal.moveDown", |ctx, args| crate::terminal::move_down(ctx, args));
    registry.register("terminal.moveRight", |ctx, args| crate::terminal::move_right(ctx, args));
    registry.register("terminal.moveLeft", |ctx, args| crate::terminal::move_left(ctx, args));
    registry.register("terminal.saveCursor", |ctx, args| crate::terminal::save_cursor(ctx, args));
    registry.register("terminal.restoreCursor", |ctx, args| crate::terminal::restore_cursor(ctx, args));
    registry.register("terminal.hideCursor", |ctx, args| crate::terminal::hide_cursor(ctx, args));
    registry.register("terminal.showCursor", |ctx, args| crate::terminal::show_cursor(ctx, args));
    // Screen control
    registry.register("terminal.clearScreen", |ctx, args| crate::terminal::clear_screen(ctx, args));
    registry.register("terminal.clearLine", |ctx, args| crate::terminal::clear_line(ctx, args));
    registry.register("terminal.clearToEndOfLine", |ctx, args| crate::terminal::clear_to_end_of_line(ctx, args));
    registry.register("terminal.clearToEndOfScreen", |ctx, args| crate::terminal::clear_to_end_of_screen(ctx, args));
}

fn register_ws(registry: &mut NativeFunctionRegistry) {
    // Client
    registry.register("ws.connect", |ctx, args| crate::ws::connect(ctx, args));
    registry.register("ws.connectWithProtocols", |ctx, args| crate::ws::connect_with_protocols(ctx, args));
    // Server
    registry.register("ws.serverCreate", |ctx, args| crate::ws::server_create(ctx, args));
    registry.register("ws.serverAccept", |ctx, args| crate::ws::server_accept(ctx, args));
    registry.register("ws.serverClose", |ctx, args| crate::ws::server_close(ctx, args));
    registry.register("ws.serverAddr", |ctx, args| crate::ws::server_addr(ctx, args));
    // Send/Receive
    registry.register("ws.send", |ctx, args| crate::ws::send(ctx, args));
    registry.register("ws.sendBytes", |ctx, args| crate::ws::send_bytes(ctx, args));
    registry.register("ws.receive", |ctx, args| crate::ws::receive(ctx, args));
    registry.register("ws.receiveBytes", |ctx, args| crate::ws::receive_bytes(ctx, args));
    // Close
    registry.register("ws.close", |ctx, args| crate::ws::close(ctx, args));
    registry.register("ws.closeWithCode", |ctx, args| crate::ws::close_with_code(ctx, args));
    // Info
    registry.register("ws.isOpen", |ctx, args| crate::ws::is_open(ctx, args));
    registry.register("ws.remoteAddr", |ctx, args| crate::ws::remote_addr(ctx, args));
    registry.register("ws.protocol", |ctx, args| crate::ws::protocol(ctx, args));
}

fn register_readline(registry: &mut NativeFunctionRegistry) {
    registry.register("readline.new", |ctx, args| crate::readline::readline_new(ctx, args));
    registry.register("readline.prompt", |ctx, args| crate::readline::readline_prompt(ctx, args));
    registry.register("readline.addHistory", |ctx, args| crate::readline::readline_add_history(ctx, args));
    registry.register("readline.loadHistory", |ctx, args| crate::readline::readline_load_history(ctx, args));
    registry.register("readline.saveHistory", |ctx, args| crate::readline::readline_save_history(ctx, args));
    registry.register("readline.clearHistory", |ctx, args| crate::readline::readline_clear_history(ctx, args));
    registry.register("readline.historySize", |ctx, args| crate::readline::readline_history_size(ctx, args));
    registry.register("readline.close", |ctx, args| crate::readline::readline_close(ctx, args));
    registry.register("readline.simplePrompt", |ctx, args| crate::readline::readline_simple_prompt(ctx, args));
    registry.register("readline.confirm", |ctx, args| crate::readline::readline_confirm(ctx, args));
    registry.register("readline.password", |ctx, args| crate::readline::readline_password(ctx, args));
    registry.register("readline.select", |ctx, args| crate::readline::readline_select(ctx, args));
}

fn register_glob(registry: &mut NativeFunctionRegistry) {
    registry.register("glob.find", |ctx, args| crate::glob_mod::glob_find(ctx, args));
    registry.register("glob.findInDir", |ctx, args| crate::glob_mod::glob_find_in_dir(ctx, args));
    registry.register("glob.matches", |ctx, args| crate::glob_mod::glob_matches(ctx, args));
}

fn register_archive(registry: &mut NativeFunctionRegistry) {
    // Tar
    registry.register("archive.tarCreate", |ctx, args| crate::archive::tar_create(ctx, args));
    registry.register("archive.tarExtract", |ctx, args| crate::archive::tar_extract(ctx, args));
    registry.register("archive.tarList", |ctx, args| crate::archive::tar_list(ctx, args));
    // Tar.gz
    registry.register("archive.tgzCreate", |ctx, args| crate::archive::tgz_create(ctx, args));
    registry.register("archive.tgzExtract", |ctx, args| crate::archive::tgz_extract(ctx, args));
    // Zip
    registry.register("archive.zipCreate", |ctx, args| crate::archive::zip_create(ctx, args));
    registry.register("archive.zipExtract", |ctx, args| crate::archive::zip_extract(ctx, args));
    registry.register("archive.zipList", |ctx, args| crate::archive::zip_list(ctx, args));
}

fn register_watch(registry: &mut NativeFunctionRegistry) {
    registry.register("watch.create", |ctx, args| crate::watch::watch_create(ctx, args));
    registry.register("watch.createRecursive", |ctx, args| crate::watch::watch_create_recursive(ctx, args));
    registry.register("watch.nextEvent", |ctx, args| crate::watch::watch_next_event(ctx, args));
    registry.register("watch.addPath", |ctx, args| crate::watch::watch_add_path(ctx, args));
    registry.register("watch.removePath", |ctx, args| crate::watch::watch_remove_path(ctx, args));
    registry.register("watch.close", |ctx, args| crate::watch::watch_close(ctx, args));
}
