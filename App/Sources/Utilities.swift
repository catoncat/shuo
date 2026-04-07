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
        "  hj-voice snapshot [timeoutSeconds]",
        "  hj-voice settings-ui",
        "  hj-voice set-shortcut <option-left|option-right|right-command> [startEnabled] [globalEnabled]",
        "  hj-voice start-mic <deviceId>",
        "  hj-voice stop-mic [deviceId]",
        "  hj-voice press <option-left|option-right|right-command> <long|double>",
        "  hj-voice press-raw <keyCode> <holdMs> [flagsInt]",
        "  hj-voice input-list",
        "  hj-voice input-select <sourceIdOrBundleId> [inputModeId]",
        "  hj-voice context [maxChars]",
        "  hj-voice shared-contract-check",
        "  hj-voice timeline-tail [count]",
        "  hj-voice timeline-sessions [count]",
        "  hj-voice app [--transport legacy-local-ws|direct-frontier] [--server-url url] [--partial-interval-ms n] [--frontier-token token] [--frontier-app-key key] [--bootstrap-env path] [--auth-cache path] [--desktop-session-env path] [--enable-mac-live-auth] [--mac-live-token-script path] [--disable-android-vdevice-auth] [--helper-bin path] [--no-hotkey] [--no-inject-final]",
        "  hj-voice engine-smoke [--timeout seconds] [--max-chars n] [--transport legacy-local-ws|direct-frontier] [--server-url url] [--partial-interval-ms n] [--frontier-token token] [--frontier-app-key key] [--bootstrap-env path] [--auth-cache path] [--desktop-session-env path] [--enable-mac-live-auth] [--mac-live-token-script path] [--disable-android-vdevice-auth] [--helper-bin path] [--no-warmup]",
        "  hj-voice engine-record [--timeout seconds] [--hold seconds] [--max-chars n] [--trigger name] [--transport legacy-local-ws|direct-frontier] [--server-url url] [--partial-interval-ms n] [--frontier-token token] [--frontier-app-key key] [--bootstrap-env path] [--auth-cache path] [--desktop-session-env path] [--enable-mac-live-auth] [--mac-live-token-script path] [--disable-android-vdevice-auth] [--helper-bin path] [--no-warmup] [--cancel|--stop]",
        "  hj-voice engine-refresh-auth [--timeout seconds] [--transport legacy-local-ws|direct-frontier] [--server-url url] [--partial-interval-ms n] [--frontier-token token] [--frontier-app-key key] [--bootstrap-env path] [--auth-cache path] [--desktop-session-env path] [--enable-mac-live-auth] [--mac-live-token-script path] [--disable-android-vdevice-auth] [--helper-bin path]",
        "examples:",
        "  hj-voice snapshot 2.0",
        "  hj-voice settings-ui",
        "  hj-voice set-shortcut option-left 1 1",
        "  hj-voice start-mic BuiltInMicrophoneDevice",
        "  hj-voice press option-left long",
        "  hj-voice press-raw 54 700 0",
        "  hj-voice input-select com.apple.keylayout.ABC",
        "  hj-voice context 256",
        "  hj-voice shared-contract-check",
        "  hj-voice timeline-tail 120",
        "  hj-voice timeline-sessions 10",
        "  hj-voice app --transport direct-frontier",
        "  hj-voice engine-smoke --timeout 12 --no-warmup",
        "  hj-voice engine-record --hold 1.0 --cancel",
        "  hj-voice engine-smoke --transport direct-frontier --bootstrap-env captures/.../bootstrap.env",
        "  hj-voice engine-refresh-auth --transport direct-frontier --auth-cache ~/Library/Application\\ Support/hj-dictation/frontier_auth.json",
        "  hj-voice engine-smoke --transport direct-frontier --desktop-session-env ~/Library/Application\\ Support/hj-dictation/desktop_session.env",
        "  hj-voice engine-refresh-auth --transport direct-frontier --enable-mac-live-auth",
    ].joined(separator: "\n")
}
