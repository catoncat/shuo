import Foundation

struct EngineContextSnapshot: Encodable {
    let frontmostBundleId: String?
    let textBeforeCursor: String
    let textAfterCursor: String
    let cursorPosition: Int
    let captureSource: String
    let capturedAtMs: UInt64

    enum CodingKeys: String, CodingKey {
        case frontmostBundleId = "frontmost_bundle_id"
        case textBeforeCursor = "text_before_cursor"
        case textAfterCursor = "text_after_cursor"
        case cursorPosition = "cursor_position"
        case captureSource = "capture_source"
        case capturedAtMs = "captured_at_ms"
    }
}

struct EngineHostCommand: Encodable {
    let type: String
    let protocolVersion: Int?
    let force: Bool?
    let sessionId: String?
    let trigger: String?
    let contextSnapshot: EngineContextSnapshot?

    enum CodingKeys: String, CodingKey {
        case type
        case protocolVersion = "protocol_version"
        case force
        case sessionId = "session_id"
        case trigger
        case contextSnapshot = "context_snapshot"
    }

    static func hello() -> Self {
        Self(
            type: "hello",
            protocolVersion: 1,
            force: nil,
            sessionId: nil,
            trigger: nil,
            contextSnapshot: nil
        )
    }

    static func updateContext(_ snapshot: EngineContextSnapshot) -> Self {
        Self(
            type: "update_context",
            protocolVersion: nil,
            force: nil,
            sessionId: nil,
            trigger: nil,
            contextSnapshot: snapshot
        )
    }

    static func warmup(force: Bool = false) -> Self {
        Self(
            type: "warmup",
            protocolVersion: nil,
            force: force,
            sessionId: nil,
            trigger: nil,
            contextSnapshot: nil
        )
    }

    static func startRecording(
        sessionId: String,
        trigger: String,
        contextSnapshot: EngineContextSnapshot
    ) -> Self {
        Self(
            type: "start_recording",
            protocolVersion: nil,
            force: nil,
            sessionId: sessionId,
            trigger: trigger,
            contextSnapshot: contextSnapshot
        )
    }

    static func stopRecording() -> Self {
        Self(
            type: "stop_recording",
            protocolVersion: nil,
            force: nil,
            sessionId: nil,
            trigger: nil,
            contextSnapshot: nil
        )
    }

    static func cancelRecording() -> Self {
        Self(
            type: "cancel_recording",
            protocolVersion: nil,
            force: nil,
            sessionId: nil,
            trigger: nil,
            contextSnapshot: nil
        )
    }

    static func exportDiagnostics() -> Self {
        Self(
            type: "export_diagnostics",
            protocolVersion: nil,
            force: nil,
            sessionId: nil,
            trigger: nil,
            contextSnapshot: nil
        )
    }

    static func refreshAuth() -> Self {
        Self(
            type: "refresh_auth",
            protocolVersion: nil,
            force: nil,
            sessionId: nil,
            trigger: nil,
            contextSnapshot: nil
        )
    }

    static func shutdown() -> Self {
        Self(
            type: "shutdown",
            protocolVersion: nil,
            force: nil,
            sessionId: nil,
            trigger: nil,
            contextSnapshot: nil
        )
    }
}

struct EngineEventRecord: Encodable {
    let type: String?
    let raw: String
    let jsonObject: [String: Any]?
    let receivedAtMs: UInt64

    enum CodingKeys: String, CodingKey {
        case type
        case raw
        case receivedAtMs = "received_at_ms"
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encodeIfPresent(type, forKey: .type)
        try container.encode(raw, forKey: .raw)
        try container.encode(receivedAtMs, forKey: .receivedAtMs)
    }
}


private func timelineCommandFields(_ command: EngineHostCommand) -> [String: Any] {
    var fields: [String: Any] = [:]
    if let force = command.force {
        fields["force"] = force
    }
    if let sessionId = command.sessionId {
        fields["session_id"] = sessionId
    }
    if let trigger = command.trigger {
        fields["trigger"] = trigger
    }
    if let context = command.contextSnapshot {
        fields["frontmost_bundle_id"] = context.frontmostBundleId ?? NSNull()
        fields["text_before_chars"] = context.textBeforeCursor.count
        fields["text_after_chars"] = context.textAfterCursor.count
        fields["cursor_position"] = context.cursorPosition
        fields["capture_source"] = context.captureSource
        fields["captured_at_ms"] = context.capturedAtMs
    }
    return fields
}

private func timelineEventFields(_ event: EngineEventRecord) -> [String: Any] {
    var fields: [String: Any] = [
        "received_at_ms": event.receivedAtMs,
    ]
    guard let object = event.jsonObject else {
        fields["raw_preview"] = runtimeTimelineTextPreview(event.raw, limit: 240)
        return fields
    }
    if let sessionId = object["session_id"] {
        fields["session_id"] = sessionId
    }
    switch event.type {
    case "ready":
        fields["protocol_version"] = object["protocol_version"] ?? NSNull()
        fields["session_state"] = object["session_state"] ?? NSNull()
        fields["auth_state"] = object["auth_state"] ?? NSNull()
        fields["transport"] = object["transport"] ?? NSNull()
    case "recording_started", "recording_stopped":
        fields["utterance_id"] = object["utterance_id"] ?? NSNull()
        fields["reason"] = object["reason"] ?? NSNull()
        fields["trigger"] = object["trigger"] ?? NSNull()
    case "partial", "final":
        fields["utterance_id"] = object["utterance_id"] ?? NSNull()
        if let text = object["text"] as? String {
            for (key, value) in runtimeTimelineTextFields(text) {
                fields[key] = value
            }
        }
    case "auth_state":
        fields["state"] = object["state"] ?? NSNull()
        fields["source"] = object["source"] ?? NSNull()
        fields["expires_at_ms"] = object["expires_at_ms"] ?? NSNull()
    case "metrics":
        let name = object["name"] as? String ?? "unknown"
        fields["name"] = name
        if name != "engine_snapshot", let value = object["value"] {
            fields["value"] = value
        }
    case "error", "fatal":
        fields["code"] = object["code"] ?? NSNull()
        fields["message"] = object["message"] ?? NSNull()
        fields["recoverable"] = object["recoverable"] ?? NSNull()
    default:
        fields["raw_preview"] = runtimeTimelineTextPreview(event.raw, limit: 240)
    }
    return fields
}

struct EngineSmokeOptions {
    var timeoutSeconds: TimeInterval = 8.0
    var maxChars: Int = 256
    var serverURL: String = "ws://127.0.0.1:8765"
    var partialIntervalMs: Int = 120
    var transport = "direct-frontier"
    var warmup = true
    var helperBin: String?
    var frontierToken: String?
    var frontierAppKey: String?
    var bootstrapEnv: String?
    var authCache: String?
    var desktopSessionEnv: String?
    var enableMacLiveAuth = false
    var macLiveTokenScript: String?
    var disableAndroidVdeviceAuth = false

    static func parse(_ args: [String]) throws -> Self {
        var options = Self()
        var index = 0
        while index < args.count {
            let arg = args[index]
            switch arg {
            case "--timeout":
                index += 1
                guard index < args.count, let value = Double(args[index]), value > 0 else {
                    throw CLIError.invalid("invalid --timeout")
                }
                options.timeoutSeconds = value
            case "--max-chars":
                index += 1
                guard index < args.count, let value = Int(args[index]), value >= 0 else {
                    throw CLIError.invalid("invalid --max-chars")
                }
                options.maxChars = value
            case "--server-url":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --server-url value")
                }
                options.serverURL = args[index]
            case "--partial-interval-ms":
                index += 1
                guard index < args.count, let value = Int(args[index]), value >= 0 else {
                    throw CLIError.invalid("invalid --partial-interval-ms")
                }
                options.partialIntervalMs = value
            case "--transport":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --transport value")
                }
                options.transport = args[index]
            case "--helper-bin":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --helper-bin value")
                }
                options.helperBin = args[index]
            case "--frontier-token":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --frontier-token value")
                }
                options.frontierToken = args[index]
            case "--frontier-app-key":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --frontier-app-key value")
                }
                options.frontierAppKey = args[index]
            case "--bootstrap-env":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --bootstrap-env value")
                }
                options.bootstrapEnv = args[index]
            case "--auth-cache":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --auth-cache value")
                }
                options.authCache = args[index]
            case "--desktop-session-env":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --desktop-session-env value")
                }
                options.desktopSessionEnv = args[index]
            case "--enable-mac-live-auth":
                options.enableMacLiveAuth = true
            case "--mac-live-token-script":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --mac-live-token-script value")
                }
                options.macLiveTokenScript = args[index]
            case "--disable-android-vdevice-auth":
                options.disableAndroidVdeviceAuth = true
            case "--no-warmup":
                options.warmup = false
            default:
                throw CLIError.usage(usage())
            }
            index += 1
        }
        return options
    }
}

struct EngineRecordOptions {
    var timeoutSeconds: TimeInterval = 15.0
    var maxChars: Int = 256
    var serverURL: String = "ws://127.0.0.1:8765"
    var partialIntervalMs: Int = 120
    var transport = "direct-frontier"
    var holdSeconds: TimeInterval = 1.2
    var warmup = true
    var cancel = true
    var trigger = "swift_host_cli"
    var helperBin: String?
    var frontierToken: String?
    var frontierAppKey: String?
    var bootstrapEnv: String?
    var authCache: String?
    var desktopSessionEnv: String?
    var enableMacLiveAuth = false
    var macLiveTokenScript: String?
    var disableAndroidVdeviceAuth = false

    static func parse(_ args: [String]) throws -> Self {
        var options = Self()
        var index = 0
        while index < args.count {
            let arg = args[index]
            switch arg {
            case "--timeout":
                index += 1
                guard index < args.count, let value = Double(args[index]), value > 0 else {
                    throw CLIError.invalid("invalid --timeout")
                }
                options.timeoutSeconds = value
            case "--max-chars":
                index += 1
                guard index < args.count, let value = Int(args[index]), value >= 0 else {
                    throw CLIError.invalid("invalid --max-chars")
                }
                options.maxChars = value
            case "--server-url":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --server-url value")
                }
                options.serverURL = args[index]
            case "--partial-interval-ms":
                index += 1
                guard index < args.count, let value = Int(args[index]), value >= 0 else {
                    throw CLIError.invalid("invalid --partial-interval-ms")
                }
                options.partialIntervalMs = value
            case "--transport":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --transport value")
                }
                options.transport = args[index]
            case "--hold":
                index += 1
                guard index < args.count, let value = Double(args[index]), value > 0 else {
                    throw CLIError.invalid("invalid --hold")
                }
                options.holdSeconds = value
            case "--trigger":
                index += 1
                guard index < args.count, !args[index].isEmpty else {
                    throw CLIError.invalid("missing --trigger value")
                }
                options.trigger = args[index]
            case "--helper-bin":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --helper-bin value")
                }
                options.helperBin = args[index]
            case "--frontier-token":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --frontier-token value")
                }
                options.frontierToken = args[index]
            case "--frontier-app-key":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --frontier-app-key value")
                }
                options.frontierAppKey = args[index]
            case "--bootstrap-env":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --bootstrap-env value")
                }
                options.bootstrapEnv = args[index]
            case "--auth-cache":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --auth-cache value")
                }
                options.authCache = args[index]
            case "--desktop-session-env":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --desktop-session-env value")
                }
                options.desktopSessionEnv = args[index]
            case "--enable-mac-live-auth":
                options.enableMacLiveAuth = true
            case "--mac-live-token-script":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --mac-live-token-script value")
                }
                options.macLiveTokenScript = args[index]
            case "--disable-android-vdevice-auth":
                options.disableAndroidVdeviceAuth = true
            case "--no-warmup":
                options.warmup = false
            case "--stop":
                options.cancel = false
            case "--cancel":
                options.cancel = true
            default:
                throw CLIError.usage(usage())
            }
            index += 1
        }
        return options
    }
}

struct EngineLaunchInfo: Encodable {
    let mode: String
    let executable: String
    let arguments: [String]
}

struct EngineSmokeReport: Encodable {
    let launcher: EngineLaunchInfo
    let contextSnapshot: EngineContextSnapshot
    let sentCommands: [String]
    let observedEvents: [EngineEventRecord]
    let stderrLines: [String]
    let initialReadyObserved: Bool
    let helloReadyObserved: Bool
    let warmupRequested: Bool
    let exitedCleanly: Bool
    let terminationStatus: Int32?
}

struct EngineRecordReport: Encodable {
    let launcher: EngineLaunchInfo
    let sessionId: String
    let trigger: String
    let contextSnapshot: EngineContextSnapshot
    let sentCommands: [String]
    let observedEvents: [EngineEventRecord]
    let stderrLines: [String]
    let initialReadyObserved: Bool
    let helloReadyObserved: Bool
    let recordingStartedObserved: Bool
    let stopAcknowledgedEventType: String?
    let terminalEventType: String?
    let warmupRequested: Bool
    let stopMode: String
    let exitedCleanly: Bool
    let terminationStatus: Int32?
}

struct EngineRefreshAuthOptions {
    var timeoutSeconds: TimeInterval = 12.0
    var serverURL: String = "ws://127.0.0.1:8765"
    var partialIntervalMs: Int = 120
    var transport = "direct-frontier"
    var helperBin: String?
    var frontierToken: String?
    var frontierAppKey: String?
    var bootstrapEnv: String?
    var authCache: String?
    var desktopSessionEnv: String?
    var enableMacLiveAuth = false
    var macLiveTokenScript: String?
    var disableAndroidVdeviceAuth = false

    static func parse(_ args: [String]) throws -> Self {
        var options = Self()
        var index = 0
        while index < args.count {
            let arg = args[index]
            switch arg {
            case "--timeout":
                index += 1
                guard index < args.count, let value = Double(args[index]), value > 0 else {
                    throw CLIError.invalid("invalid --timeout")
                }
                options.timeoutSeconds = value
            case "--server-url":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --server-url value")
                }
                options.serverURL = args[index]
            case "--partial-interval-ms":
                index += 1
                guard index < args.count, let value = Int(args[index]), value >= 0 else {
                    throw CLIError.invalid("invalid --partial-interval-ms")
                }
                options.partialIntervalMs = value
            case "--transport":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --transport value")
                }
                options.transport = args[index]
            case "--helper-bin":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --helper-bin value")
                }
                options.helperBin = args[index]
            case "--frontier-token":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --frontier-token value")
                }
                options.frontierToken = args[index]
            case "--frontier-app-key":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --frontier-app-key value")
                }
                options.frontierAppKey = args[index]
            case "--bootstrap-env":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --bootstrap-env value")
                }
                options.bootstrapEnv = args[index]
            case "--auth-cache":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --auth-cache value")
                }
                options.authCache = args[index]
            case "--desktop-session-env":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --desktop-session-env value")
                }
                options.desktopSessionEnv = args[index]
            case "--enable-mac-live-auth":
                options.enableMacLiveAuth = true
            case "--mac-live-token-script":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --mac-live-token-script value")
                }
                options.macLiveTokenScript = args[index]
            case "--disable-android-vdevice-auth":
                options.disableAndroidVdeviceAuth = true
            default:
                throw CLIError.usage(usage())
            }
            index += 1
        }
        return options
    }
}

struct EngineRefreshAuthReport: Encodable {
    let launcher: EngineLaunchInfo
    let sentCommands: [String]
    let observedEvents: [EngineEventRecord]
    let stderrLines: [String]
    let initialReadyObserved: Bool
    let helloReadyObserved: Bool
    let refreshRequested: Bool
    let terminalEventType: String?
    let exitedCleanly: Bool
    let terminationStatus: Int32?
}

final class EngineProcessBridge {
    private let launcher: EngineLaunchInfo
    private let currentDirectoryURL: URL
    private let process = Process()
    private let stdoutPipe = Pipe()
    private let stderrPipe = Pipe()
    private let stdinPipe = Pipe()
    private let condition = NSCondition()

    private var observedEvents: [EngineEventRecord] = []
    private var stderrLines: [String] = []
    private var sentCommands: [String] = []
    private var stdoutClosed = false
    private var stderrClosed = false
    var onEvent: ((EngineEventRecord) -> Void)?
    var onStderrLine: ((String) -> Void)?
    var onTermination: ((Int32) -> Void)?

    init(launcher: EngineLaunchInfo, currentDirectoryURL: URL) {
        self.launcher = launcher
        self.currentDirectoryURL = currentDirectoryURL
    }

    func start() throws {
        process.currentDirectoryURL = currentDirectoryURL
        process.executableURL = URL(fileURLWithPath: launcher.executable)
        process.arguments = launcher.arguments
        process.standardInput = stdinPipe
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe
        process.terminationHandler = { [weak self] process in
            RuntimeTimeline.shared.record("helper", "process_terminated", fields: [
                "status": process.terminationStatus,
                "reason": process.terminationReason == .exit ? "exit" : "uncaught_signal",
            ])
            self?.condition.lock()
            self?.condition.broadcast()
            self?.condition.unlock()
            self?.onTermination?(process.terminationStatus)
        }
        try process.run()
        RuntimeTimeline.shared.record("helper", "process_started", fields: [
            "executable": launcher.executable,
            "arguments": launcher.arguments,
            "timeline_file": RuntimeTimeline.shared.currentFilePath(),
        ])
        pumpLines(from: stdoutPipe.fileHandleForReading, isStdout: true)
        pumpLines(from: stderrPipe.fileHandleForReading, isStdout: false)
    }

    func send(_ command: EngineHostCommand) throws {
        let data = try jsonLine(command)
        try stdinPipe.fileHandleForWriting.write(contentsOf: data)
        let line = String(decoding: data.dropLast(), as: UTF8.self)
        RuntimeTimeline.shared.record("engine_command", command.type, fields: timelineCommandFields(command))
        condition.lock()
        sentCommands.append(line)
        condition.unlock()
    }

    func waitForEvent(
        after cursor: inout Int,
        matching types: Set<String>,
        timeout: TimeInterval
    ) -> EngineEventRecord? {
        waitForRecord(after: &cursor, timeout: timeout) { event in
            guard let type = event.type else { return false }
            return types.contains(type)
        }
    }

    func waitForRecord(
        after cursor: inout Int,
        timeout: TimeInterval,
        matching predicate: (EngineEventRecord) -> Bool
    ) -> EngineEventRecord? {
        let deadline = Date().addingTimeInterval(timeout)
        condition.lock()
        defer { condition.unlock() }

        while true {
            if cursor < observedEvents.count {
                for index in cursor..<observedEvents.count {
                    let event = observedEvents[index]
                    if predicate(event) {
                        cursor = index + 1
                        return event
                    }
                }
                cursor = observedEvents.count
            }
            if !process.isRunning && stdoutClosed {
                return nil
            }
            let remaining = deadline.timeIntervalSinceNow
            if remaining <= 0 {
                return nil
            }
            condition.wait(until: Date().addingTimeInterval(min(remaining, 0.2)))
        }
    }

    func waitForExit(timeout: TimeInterval) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while process.isRunning && Date() < deadline {
            Thread.sleep(forTimeInterval: 0.05)
        }
        if process.isRunning {
            process.terminate()
            process.waitUntilExit()
            return false
        }
        return process.terminationReason == .exit && process.terminationStatus == 0
    }

    func snapshot() -> (events: [EngineEventRecord], stderr: [String], sent: [String]) {
        condition.lock()
        defer { condition.unlock() }
        return (observedEvents, stderrLines, sentCommands)
    }

    var terminationStatus: Int32? {
        process.isRunning ? nil : process.terminationStatus
    }

    var isRunning: Bool {
        process.isRunning
    }

    func cleanup() {
        if process.isRunning {
            process.terminate()
            process.waitUntilExit()
        }
    }

    private func pumpLines(from handle: FileHandle, isStdout: Bool) {
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self else { return }
            var buffer = Data()
            while true {
                let chunk = handle.availableData
                if chunk.isEmpty {
                    break
                }
                buffer.append(chunk)
                while let newlineIndex = buffer.firstIndex(of: 0x0a) {
                    let lineData = buffer.prefix(upTo: newlineIndex)
                    buffer.removeSubrange(...newlineIndex)
                    let line = String(decoding: lineData, as: UTF8.self)
                    self.record(line: line, isStdout: isStdout)
                }
            }
            if !buffer.isEmpty {
                let line = String(decoding: buffer, as: UTF8.self)
                self.record(line: line, isStdout: isStdout)
            }
            self.condition.lock()
            if isStdout {
                self.stdoutClosed = true
            } else {
                self.stderrClosed = true
            }
            self.condition.broadcast()
            self.condition.unlock()
        }
    }

    private func record(line rawLine: String, isStdout: Bool) {
        let line = rawLine.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !line.isEmpty else { return }
        let event: EngineEventRecord?
        condition.lock()
        if isStdout {
            let object = decodeJSONObject(from: line)
            let current = EngineEventRecord(
                type: object?["type"] as? String,
                raw: line,
                jsonObject: object,
                receivedAtMs: runtimeTimelineNowMs()
            )
            observedEvents.append(current)
            if current.type != "audio_level" {
                RuntimeTimeline.shared.record("engine_event", current.type ?? "unknown", fields: timelineEventFields(current))
            }
            event = current
        } else {
            stderrLines.append(line)
            RuntimeTimeline.shared.record("helper_stderr", "line", fields: [
                "line": runtimeTimelineTextPreview(line, limit: 240),
            ])
            event = nil
        }
        condition.broadcast()
        condition.unlock()
        if let event {
            onEvent?(event)
        } else {
            onStderrLine?(line)
        }
    }

    private func decodeJSONObject(from line: String) -> [String: Any]? {
        guard let data = line.data(using: .utf8) else {
            return nil
        }
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
    }
}

func runEngineSmoke(options: EngineSmokeOptions) throws {
    let currentDirectoryURL = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
    let launcher = resolveEngineLauncher(
        currentDirectoryURL: currentDirectoryURL,
        helperBin: options.helperBin,
        transport: options.transport,
        serverURL: options.serverURL,
        partialIntervalMs: options.partialIntervalMs,
        frontierToken: options.frontierToken,
        frontierAppKey: options.frontierAppKey,
        bootstrapEnv: options.bootstrapEnv,
        authCache: options.authCache,
        desktopSessionEnv: options.desktopSessionEnv,
        enableMacLiveAuth: options.enableMacLiveAuth,
        macLiveTokenScript: options.macLiveTokenScript,
        disableAndroidVdeviceAuth: options.disableAndroidVdeviceAuth
    )
    let snapshot = makeEngineContextSnapshot(maxChars: options.maxChars)
    let bridge = EngineProcessBridge(launcher: launcher, currentDirectoryURL: currentDirectoryURL)
    defer { bridge.cleanup() }

    try bridge.start()

    var cursor = 0
    let initialReady = bridge.waitForEvent(
        after: &cursor,
        matching: ["ready"],
        timeout: options.timeoutSeconds
    )
    guard initialReady != nil else {
        let snapshot = bridge.snapshot()
        throw CLIError.timeout("engine ready timeout; stderr=\(snapshot.stderr.joined(separator: " | "))")
    }

    try bridge.send(.hello())
    let helloReady = bridge.waitForEvent(
        after: &cursor,
        matching: ["ready"],
        timeout: options.timeoutSeconds
    )
    guard helloReady != nil else {
        let snapshot = bridge.snapshot()
        throw CLIError.timeout("engine hello ack timeout; stderr=\(snapshot.stderr.joined(separator: " | "))")
    }

    try bridge.send(.updateContext(snapshot))
    if options.warmup {
        try bridge.send(.warmup())
        _ = bridge.waitForRecord(after: &cursor, timeout: min(options.timeoutSeconds, 4.0)) { event in
            if event.type == "auth_state" || event.type == "error" || event.type == "fatal" {
                return true
            }
            return event.type == "metrics"
                && (event.raw.contains("\"name\":\"warmed\"")
                    || event.raw.contains("\"name\":\"transport_ready\""))
        }
    }
    try bridge.send(.exportDiagnostics())
    _ = bridge.waitForEvent(
        after: &cursor,
        matching: ["metrics", "error"],
        timeout: min(options.timeoutSeconds, 2.0)
    )
    try bridge.send(.shutdown())
    let exitedCleanly = bridge.waitForExit(timeout: options.timeoutSeconds)
    let output = bridge.snapshot()
    print(try toJSON(EngineSmokeReport(
        launcher: launcher,
        contextSnapshot: snapshot,
        sentCommands: output.sent,
        observedEvents: output.events,
        stderrLines: output.stderr,
        initialReadyObserved: initialReady != nil,
        helloReadyObserved: helloReady != nil,
        warmupRequested: options.warmup,
        exitedCleanly: exitedCleanly,
        terminationStatus: bridge.terminationStatus
    )))
}

func runEngineRecord(options: EngineRecordOptions) throws {
    let currentDirectoryURL = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
    let launcher = resolveEngineLauncher(
        currentDirectoryURL: currentDirectoryURL,
        helperBin: options.helperBin,
        transport: options.transport,
        serverURL: options.serverURL,
        partialIntervalMs: options.partialIntervalMs,
        frontierToken: options.frontierToken,
        frontierAppKey: options.frontierAppKey,
        bootstrapEnv: options.bootstrapEnv,
        authCache: options.authCache,
        desktopSessionEnv: options.desktopSessionEnv,
        enableMacLiveAuth: options.enableMacLiveAuth,
        macLiveTokenScript: options.macLiveTokenScript,
        disableAndroidVdeviceAuth: options.disableAndroidVdeviceAuth
    )
    let snapshot = makeEngineContextSnapshot(maxChars: options.maxChars)
    let bridge = EngineProcessBridge(launcher: launcher, currentDirectoryURL: currentDirectoryURL)
    defer { bridge.cleanup() }

    try bridge.start()

    var cursor = 0
    let initialReady = bridge.waitForEvent(after: &cursor, matching: ["ready"], timeout: options.timeoutSeconds)
    guard initialReady != nil else {
        let snapshot = bridge.snapshot()
        throw CLIError.timeout("engine ready timeout; stderr=\(snapshot.stderr.joined(separator: " | "))")
    }

    try bridge.send(.hello())
    let helloReady = bridge.waitForEvent(after: &cursor, matching: ["ready"], timeout: options.timeoutSeconds)
    guard helloReady != nil else {
        let snapshot = bridge.snapshot()
        throw CLIError.timeout("engine hello ack timeout; stderr=\(snapshot.stderr.joined(separator: " | "))")
    }

    try bridge.send(.updateContext(snapshot))
    if options.warmup {
        try bridge.send(.warmup())
        _ = bridge.waitForRecord(after: &cursor, timeout: min(options.timeoutSeconds, 4.0)) { event in
            if event.type == "auth_state" || event.type == "error" || event.type == "fatal" {
                return true
            }
            return event.type == "metrics"
                && (event.raw.contains("\"name\":\"warmed\"")
                    || event.raw.contains("\"name\":\"transport_ready\""))
        }
    }

    let sessionId = "swift-host-\(UUID().uuidString.lowercased())"
    try bridge.send(.startRecording(
        sessionId: sessionId,
        trigger: options.trigger,
        contextSnapshot: snapshot
    ))
    let recordingStarted = bridge.waitForEvent(
        after: &cursor,
        matching: ["recording_started", "error", "fatal"],
        timeout: options.timeoutSeconds
    )
    let recordingStartedObserved = recordingStarted?.type == "recording_started"

    if recordingStartedObserved {
        Thread.sleep(forTimeInterval: options.holdSeconds)
        try bridge.send(options.cancel ? .cancelRecording() : .stopRecording())
    }

    let stopAcknowledgedEvent: EngineEventRecord?
    let terminalEvent: EngineEventRecord?
    if options.cancel {
        stopAcknowledgedEvent = nil
        terminalEvent = bridge.waitForEvent(
            after: &cursor,
            matching: ["recording_stopped", "final"],
            timeout: options.timeoutSeconds
        ) ?? bridge.waitForEvent(
            after: &cursor,
            matching: ["error", "fatal"],
            timeout: min(options.timeoutSeconds, 2.0)
        )
    } else {
        stopAcknowledgedEvent = bridge.waitForEvent(
            after: &cursor,
            matching: ["recording_stopped", "error", "fatal"],
            timeout: options.timeoutSeconds
        )
        if let stopAcknowledgedEvent, stopAcknowledgedEvent.type == "recording_stopped" {
            terminalEvent = bridge.waitForEvent(
                after: &cursor,
                matching: ["final", "error", "fatal"],
                timeout: options.timeoutSeconds
            )
        } else {
            terminalEvent = stopAcknowledgedEvent
        }
    }

    try bridge.send(.shutdown())
    let exitedCleanly = bridge.waitForExit(timeout: options.timeoutSeconds)
    let output = bridge.snapshot()
    print(try toJSON(EngineRecordReport(
        launcher: launcher,
        sessionId: sessionId,
        trigger: options.trigger,
        contextSnapshot: snapshot,
        sentCommands: output.sent,
        observedEvents: output.events,
        stderrLines: output.stderr,
        initialReadyObserved: initialReady != nil,
        helloReadyObserved: helloReady != nil,
        recordingStartedObserved: recordingStartedObserved,
        stopAcknowledgedEventType: stopAcknowledgedEvent?.type,
        terminalEventType: terminalEvent?.type,
        warmupRequested: options.warmup,
        stopMode: options.cancel ? "cancel" : "stop",
        exitedCleanly: exitedCleanly,
        terminationStatus: bridge.terminationStatus
    )))
}

func runEngineRefreshAuth(options: EngineRefreshAuthOptions) throws {
    let currentDirectoryURL = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
    let launcher = resolveEngineLauncher(
        currentDirectoryURL: currentDirectoryURL,
        helperBin: options.helperBin,
        transport: options.transport,
        serverURL: options.serverURL,
        partialIntervalMs: options.partialIntervalMs,
        frontierToken: options.frontierToken,
        frontierAppKey: options.frontierAppKey,
        bootstrapEnv: options.bootstrapEnv,
        authCache: options.authCache,
        desktopSessionEnv: options.desktopSessionEnv,
        enableMacLiveAuth: options.enableMacLiveAuth,
        macLiveTokenScript: options.macLiveTokenScript,
        disableAndroidVdeviceAuth: options.disableAndroidVdeviceAuth
    )
    let bridge = EngineProcessBridge(launcher: launcher, currentDirectoryURL: currentDirectoryURL)
    defer { bridge.cleanup() }

    try bridge.start()

    var cursor = 0
    let initialReady = bridge.waitForEvent(
        after: &cursor,
        matching: ["ready"],
        timeout: options.timeoutSeconds
    )
    guard initialReady != nil else {
        let snapshot = bridge.snapshot()
        throw CLIError.timeout("engine ready timeout; stderr=\(snapshot.stderr.joined(separator: " | "))")
    }

    try bridge.send(.hello())
    let helloReady = bridge.waitForEvent(
        after: &cursor,
        matching: ["ready"],
        timeout: options.timeoutSeconds
    )
    guard helloReady != nil else {
        let snapshot = bridge.snapshot()
        throw CLIError.timeout("engine hello ack timeout; stderr=\(snapshot.stderr.joined(separator: " | "))")
    }

    try bridge.send(.refreshAuth())
    let terminalEvent = bridge.waitForRecord(after: &cursor, timeout: options.timeoutSeconds) { event in
        if event.type == "error" || event.type == "fatal" {
            return true
        }
        guard event.type == "auth_state" else {
            return false
        }
        return event.raw.contains("\"state\":\"ready\"")
            || event.raw.contains("\"state\":\"failed\"")
            || event.raw.contains("\"state\":\"degraded\"")
    }
    try bridge.send(.exportDiagnostics())
    _ = bridge.waitForEvent(
        after: &cursor,
        matching: ["metrics", "error"],
        timeout: min(options.timeoutSeconds, 2.0)
    )
    try bridge.send(.shutdown())
    let exitedCleanly = bridge.waitForExit(timeout: options.timeoutSeconds)
    let output = bridge.snapshot()
    print(try toJSON(EngineRefreshAuthReport(
        launcher: launcher,
        sentCommands: output.sent,
        observedEvents: output.events,
        stderrLines: output.stderr,
        initialReadyObserved: initialReady != nil,
        helloReadyObserved: helloReady != nil,
        refreshRequested: true,
        terminalEventType: terminalEvent?.type,
        exitedCleanly: exitedCleanly,
        terminationStatus: bridge.terminationStatus
    )))
}

private func makeEngineContextSnapshot(maxChars: Int) -> EngineContextSnapshot {
    let context = focusedTextContext(maxChars: maxChars)
    return EngineContextSnapshot(
        frontmostBundleId: context.frontmostAppBundleId,
        textBeforeCursor: context.textBeforeCursor ?? "",
        textAfterCursor: context.textAfterCursor ?? "",
        cursorPosition: context.cursorPosition ?? 0,
        captureSource: context.captureSource,
        capturedAtMs: currentTimeMillis()
    )
}

func resolveEngineLauncher(
    currentDirectoryURL: URL,
    helperBin: String?,
    transport: String,
    serverURL: String,
    partialIntervalMs: Int,
    frontierToken: String?,
    frontierAppKey: String?,
    bootstrapEnv: String?,
    authCache: String?,
    desktopSessionEnv: String?,
    enableMacLiveAuth: Bool,
    macLiveTokenScript: String?,
    disableAndroidVdeviceAuth: Bool
) -> EngineLaunchInfo {
    var helperArguments = [
        "--mode", "stdio-engine",
        "--transport", transport,
        "--frontier-profile", "current-opus",
        "--server-url", serverURL,
        "--partial-interval-ms", String(partialIntervalMs),
    ]
    if let frontierToken {
        helperArguments += ["--frontier-token", frontierToken]
    }
    if let frontierAppKey {
        helperArguments += ["--frontier-app-key", frontierAppKey]
    }
    if let bootstrapEnv {
        helperArguments += ["--bootstrap-env", bootstrapEnv]
    }
    if let authCache {
        helperArguments += ["--auth-cache-path", authCache]
    }
    if let desktopSessionEnv {
        helperArguments += ["--desktop-session-env", desktopSessionEnv]
    }
    if enableMacLiveAuth {
        helperArguments += ["--enable-mac-live-auth"]
    }
    if let macLiveTokenScript {
        helperArguments += ["--mac-live-token-script", macLiveTokenScript]
    }
    if disableAndroidVdeviceAuth {
        helperArguments += ["--disable-android-vdevice-auth"]
    }
    if let helperBin {
        return EngineLaunchInfo(mode: "binary", executable: helperBin, arguments: helperArguments)
    }

    for bundledHelper in bundledHelperCandidateURLs() {
        if FileManager.default.isExecutableFile(atPath: bundledHelper.path) {
            return EngineLaunchInfo(mode: "binary", executable: bundledHelper.path, arguments: helperArguments)
        }
    }

    let helperCandidates: [(binary: URL, root: URL)] = [
        (
            currentDirectoryURL.appendingPathComponent("Engine/shuo-engine/target/debug/shuo-engine"),
            currentDirectoryURL.appendingPathComponent("Engine/shuo-engine")
        ),
    ]
    for candidate in helperCandidates {
        if FileManager.default.isExecutableFile(atPath: candidate.binary.path),
           helperBinaryIsFresh(binaryPath: candidate.binary.path, helperRootURLs: [candidate.root]) {
            return EngineLaunchInfo(mode: "binary", executable: candidate.binary.path, arguments: helperArguments)
        }
    }

    let manifestCandidates = [
        currentDirectoryURL.appendingPathComponent("Engine/shuo-engine/Cargo.toml"),
    ]
    let manifestPath = manifestCandidates.first { FileManager.default.fileExists(atPath: $0.path) }?.path
        ?? manifestCandidates[0].path
    return EngineLaunchInfo(
        mode: "cargo_run",
        executable: "/usr/bin/env",
        arguments: [
            "cargo",
            "run",
            "--quiet",
            "--manifest-path",
            manifestPath,
            "--",
        ] + helperArguments
    )
}

private func helperBinaryIsFresh(binaryPath: String, helperRootURLs: [URL]) -> Bool {
    let fileManager = FileManager.default
    guard let binaryDate = try? fileManager.attributesOfItem(atPath: binaryPath)[.modificationDate] as? Date else {
        return false
    }
    for helperRootURL in helperRootURLs {
        let watchedURLs = [
            helperRootURL.appendingPathComponent("Cargo.toml"),
            helperRootURL.appendingPathComponent("Cargo.lock"),
            helperRootURL.appendingPathComponent("build.rs"),
            helperRootURL.appendingPathComponent("src"),
        ]
        for url in watchedURLs {
            var isDirectory: ObjCBool = false
            if fileManager.fileExists(atPath: url.path, isDirectory: &isDirectory), isDirectory.boolValue {
                let enumerator = fileManager.enumerator(
                    at: url,
                    includingPropertiesForKeys: [.contentModificationDateKey],
                    options: [.skipsHiddenFiles]
                )
                while let fileURL = enumerator?.nextObject() as? URL {
                    guard let values = try? fileURL.resourceValues(forKeys: [.isRegularFileKey, .contentModificationDateKey]),
                          values.isRegularFile == true else {
                        continue
                    }
                    if let modifiedAt = values.contentModificationDate, modifiedAt > binaryDate {
                        return false
                    }
                }
                continue
            }
            if let modifiedAt = try? fileManager.attributesOfItem(atPath: url.path)[.modificationDate] as? Date,
               modifiedAt > binaryDate {
                return false
            }
        }
    }
    return true
}

private func bundledHelperCandidateURLs() -> [URL] {
    var candidates: [URL] = []
    if let resourceURL = Bundle.main.resourceURL {
        candidates.append(resourceURL.appendingPathComponent("bin/shuo-engine"))
    }
    if let executableDir = Bundle.main.executableURL?.deletingLastPathComponent() {
        candidates.append(executableDir.appendingPathComponent("shuo-engine"))
    }
    return candidates
}
