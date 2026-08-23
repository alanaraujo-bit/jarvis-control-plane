// Prevents an additional console window from appearing alongside the app on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Guardrail mode (§35), checked before anything else.
    //
    // The same executable serves as the pre-tool hook for agent sessions: the
    // provider runs it once per tool call, it reads the call on stdin and
    // answers on stdout. It must not build a window, open the database, or
    // touch the single-instance lock — the running application owns all three,
    // and a hook that fought it for them would break the session it is meant to
    // be protecting.
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some(jarvis_lib::GUARDRAIL_HOOK_FLAG) {
        let snapshot = args.next().unwrap_or_default();
        std::process::exit(jarvis_lib::run_guardrail_hook(&snapshot));
    }

    jarvis_lib::run()
}
