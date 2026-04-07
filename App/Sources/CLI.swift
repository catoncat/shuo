import Carbon
import Foundation

@main
struct Main {
    static func main() {
        do {
            try run()
        } catch let error as CLIError {
            FileHandle.standardError.write(Data((error.description + "\n").utf8))
            Foundation.exit(1)
        } catch {
            FileHandle.standardError.write(Data(("unexpected error: \(error)\n").utf8))
            Foundation.exit(1)
        }
    }

    static func run() throws {
        let args = Array(CommandLine.arguments.dropFirst())
        guard let command = args.first else {
            if runningFromAppBundle() {
                runMenuBarApp(options: .init())
                return
            }
            throw CLIError.usage(usage())
        }

        let bridge = DoubaoBridge()

        switch command {
        case "settings-ui":
            runSettingsUI()

        case "snapshot":
            let timeout = args.count > 1 ? Double(args[1]) ?? 2.0 : 2.0
            let snapshots = try bridge.requestSnapshots(timeout: timeout)
            print(try toJSON(snapshots))

        case "set-shortcut":
            guard args.count >= 2, let preset = KeyPreset(cliValue: args[1]) else {
                throw CLIError.usage(usage())
            }
            let startEnabled = args.count > 2 ? try boolArg(args[2], name: "startEnabled") : true
            let globalEnabled = args.count > 3 ? try boolArg(args[3], name: "globalEnabled") : true
            bridge.postShortcutConfig(
                startEnabled: startEnabled,
                globalEnabled: globalEnabled,
                keyCode: Int(preset.keyCode),
                modifierFlags: 0,
                display: preset.display
            )

        case "start-mic":
            guard args.count >= 2 else {
                throw CLIError.usage(usage())
            }
            bridge.startMicrophone(deviceId: args[1])

        case "stop-mic":
            bridge.stopMicrophone(deviceId: args.count > 1 ? args[1] : nil)

        case "press":
            guard args.count >= 3, let preset = KeyPreset(cliValue: args[1]) else {
                throw CLIError.usage(usage())
            }
            try pressModifier(preset, mode: args[2])

        case "press-raw":
            guard args.count >= 3,
                  let keyCode = UInt16(args[1]),
                  let holdMs = useconds_t(args[2]) else {
                throw CLIError.usage(usage())
            }
            let flags = args.count > 3
                ? CGEventFlags(rawValue: UInt64(args[3]) ?? 0)
                : []
            try pressRawKey(code: CGKeyCode(keyCode), flags: flags, holdMs: holdMs)

        case "input-list":
            let sources = allInputSources().map(inputSourceInfo)
            print(try toJSON(sources))

        case "input-select":
            guard args.count >= 2 else {
                throw CLIError.usage(usage())
            }
            let info = try selectInputSource(
                matcher: args[1],
                inputModeId: args.count > 2 ? args[2] : nil
            )
            print(try toJSON(info))

        case "context":
            let maxChars = args.count > 1 ? max(0, Int(args[1]) ?? 256) : 256
            print(try toJSON(focusedTextContext(maxChars: maxChars)))

        case "shared-contract-check":
            try runSharedContractCheck()

        case "timeline-tail":
            let count = args.count > 1 ? max(0, Int(args[1]) ?? 80) : 80
            try runTimelineTail(count: count)

        case "timeline-sessions":
            let count = args.count > 1 ? max(0, Int(args[1]) ?? 20) : 20
            try runTimelineSessions(count: count)

        case "engine-smoke":
            try runEngineSmoke(options: EngineSmokeOptions.parse(Array(args.dropFirst())))

        case "engine-record":
            try runEngineRecord(options: EngineRecordOptions.parse(Array(args.dropFirst())))

        case "engine-refresh-auth":
            try runEngineRefreshAuth(options: EngineRefreshAuthOptions.parse(Array(args.dropFirst())))

        case "latency-bench":
            try runLatencyBench(options: LatencyBenchOptions.parse(Array(args.dropFirst())))

        case "app":
            runMenuBarApp(options: try AppOptions.parse(Array(args.dropFirst())))

        default:
            throw CLIError.usage(usage())
        }
    }
}

private func runningFromAppBundle() -> Bool {
    let bundleURL = Bundle.main.bundleURL
    guard bundleURL.pathExtension.lowercased() == "app" else {
        return false
    }
    return FileManager.default.fileExists(
        atPath: bundleURL.appendingPathComponent("Contents/Info.plist").path
    )
}
