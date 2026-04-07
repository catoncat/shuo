import Foundation

enum CLIError: Error, CustomStringConvertible {
    case usage(String)
    case invalid(String)

    var description: String {
        switch self {
        case .usage(let text), .invalid(let text):
            return text
        }
    }
}

func usage() -> String {
    [
        "usage:",
        "  shuo-bench [--runs n] [--phrase text] [--voice name] [--profiles csv] [--chunk-ms csv] [--timeout seconds] [--frontier-token token] [--frontier-app-key key] [--bootstrap-env path] [--auth-cache path] [--desktop-session-env path] [--enable-mac-live-auth] [--mac-live-token-script path] [--disable-android-vdevice-auth] [--helper-bin path] [--no-warmup]",
        "examples:",
        "  shuo-bench --runs 3 --chunk-ms 10,20,40",
        "  shuo-bench --profiles current-opus,android-opus --no-warmup",
    ].joined(separator: "\n")
}

func currentTimeMillis() -> UInt64 {
    UInt64(Date().timeIntervalSince1970 * 1000)
}
