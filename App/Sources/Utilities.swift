import Foundation

func toJSON<T: Encodable>(_ value: T) throws -> String {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
    let data = try encoder.encode(value)
    return String(decoding: data, as: UTF8.self)
}

func boolArg(_ raw: String, name: String) throws -> Bool {
    switch raw {
    case "1", "true", "yes":
        return true
    case "0", "false", "no":
        return false
    default:
        throw CLIError.invalid("invalid \(name): \(raw)")
    }
}

func currentTimeMillis() -> UInt64 {
    UInt64(Date().timeIntervalSince1970 * 1000)
}

func jsonLine<T: Encodable>(_ value: T) throws -> Data {
    let encoder = JSONEncoder()
    let data = try encoder.encode(value)
    return data + Data([0x0a])
}

func usage() -> String {
    [
        "usage:",
        "  shuo snapshot [timeoutSeconds]",
        "  shuo settings-ui",
        "  shuo set-shortcut <option-left|option-right|right-command> [startEnabled] [globalEnabled]",
        "  shuo start-mic <deviceId>",
        "  shuo stop-mic [deviceId]",
        "  shuo press <option-left|option-right|right-command> <long|double>",
        "  shuo press-raw <keyCode> <holdMs> [flagsInt]",
        "  shuo input-list",
        "  shuo input-select <sourceIdOrBundleId> [inputModeId]",
        "  shuo context [maxChars]",
        "  shuo shared-contract-check",
        "  shuo timeline-tail [count]",
        "  shuo timeline-sessions [count]",
        "  shuo latency-bench [--runs n] [--phrase text] [--voice name] [--profiles csv] [--chunk-ms csv] [--timeout seconds] [--frontier-token token] [--frontier-app-key key] [--bootstrap-env path] [--auth-cache path] [--desktop-session-env path] [--enable-mac-live-auth] [--mac-live-token-script path] [--disable-android-vdevice-auth] [--helper-bin path] [--no-warmup]",
        "  shuo app [--transport legacy-local-ws|direct-frontier] [--server-url url] [--partial-interval-ms n] [--frontier-token token] [--frontier-app-key key] [--bootstrap-env path] [--auth-cache path] [--desktop-session-env path] [--enable-mac-live-auth] [--mac-live-token-script path] [--disable-android-vdevice-auth] [--helper-bin path] [--no-hotkey] [--no-inject-final]",
        "  shuo engine-smoke [--timeout seconds] [--max-chars n] [--transport legacy-local-ws|direct-frontier] [--server-url url] [--partial-interval-ms n] [--frontier-token token] [--frontier-app-key key] [--bootstrap-env path] [--auth-cache path] [--desktop-session-env path] [--enable-mac-live-auth] [--mac-live-token-script path] [--disable-android-vdevice-auth] [--helper-bin path] [--no-warmup]",
        "  shuo engine-record [--timeout seconds] [--hold seconds] [--max-chars n] [--trigger name] [--transport legacy-local-ws|direct-frontier] [--server-url url] [--partial-interval-ms n] [--frontier-token token] [--frontier-app-key key] [--bootstrap-env path] [--auth-cache path] [--desktop-session-env path] [--enable-mac-live-auth] [--mac-live-token-script path] [--disable-android-vdevice-auth] [--helper-bin path] [--no-warmup] [--cancel|--stop]",
        "  shuo engine-refresh-auth [--timeout seconds] [--transport legacy-local-ws|direct-frontier] [--server-url url] [--partial-interval-ms n] [--frontier-token token] [--frontier-app-key key] [--bootstrap-env path] [--auth-cache path] [--desktop-session-env path] [--enable-mac-live-auth] [--mac-live-token-script path] [--disable-android-vdevice-auth] [--helper-bin path]",
        "examples:",
        "  shuo snapshot 2.0",
        "  shuo settings-ui",
        "  shuo set-shortcut option-left 1 1",
        "  shuo start-mic BuiltInMicrophoneDevice",
        "  shuo press option-left long",
        "  shuo press-raw 54 700 0",
        "  shuo input-select com.apple.keylayout.ABC",
        "  shuo context 256",
        "  shuo shared-contract-check",
        "  shuo timeline-tail 120",
        "  shuo timeline-sessions 10",
        "  shuo latency-bench --runs 3 --chunk-ms 10,20,40",
        "  shuo app --transport direct-frontier",
        "  shuo engine-smoke --timeout 12 --no-warmup",
        "  shuo engine-record --hold 1.0 --cancel",
        "  shuo engine-smoke --transport direct-frontier --bootstrap-env captures/.../bootstrap.env",
        "  shuo engine-refresh-auth --transport direct-frontier --auth-cache ~/Library/Application\\ Support/shuo-engine/frontier_auth.json",
        "  shuo engine-smoke --transport direct-frontier --desktop-session-env ~/Library/Application\\ Support/shuo-engine/desktop_session.env",
        "  shuo engine-refresh-auth --transport direct-frontier --enable-mac-live-auth",
    ].joined(separator: "\n")
}
