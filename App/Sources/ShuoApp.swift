import AppKit
import Carbon
import Foundation

private let systemSfxBegin = "jbl_begin_short.caf"
private let systemSfxConfirm = "jbl_confirm.caf"
private var activeFeedbackSounds: [NSSound] = []

struct AppOptions {
    var serverURL: String = "ws://127.0.0.1:8765"
    var partialIntervalMs: Int = 120
    var transport: String = "direct-frontier"
    var helperBin: String?
    var frontierToken: String?
    var frontierAppKey: String?
    var bootstrapEnv: String?
    var authCache: String?
    var desktopSessionEnv: String?
    var enableMacLiveAuth = false
    var macLiveTokenScript: String?
    var disableAndroidVdeviceAuth = false
    var enableHotkey: Bool = true
    var injectFinalText: Bool = true

    static func parse(_ args: [String]) throws -> Self {
        var options = Self()
        var index = 0
        while index < args.count {
            let arg = args[index]
            switch arg {
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
            case "--no-hotkey":
                options.enableHotkey = false
            case "--no-inject-final":
                options.injectFinalText = false
            default:
                throw CLIError.usage(usage())
            }
            index += 1
        }
        return options
    }
}

private struct ShortcutBinding {
    let keyCode: UInt16
    let flags: NSEvent.ModifierFlags
    let mode: String
    let title: String
}

private enum ShortcutSignal {
    case press
    case release
    case doubleTap
    case warmupHint
}

private func makeShortcutBinding(from config: ContextConfig) -> ShortcutBinding {
    switch config.shortcut.key {
    case "left_command":
        return ShortcutBinding(keyCode: 55, flags: .command, mode: config.shortcut.mode, title: "左⌘")
    case "right_option":
        return ShortcutBinding(keyCode: 61, flags: .option, mode: config.shortcut.mode, title: "右⌥")
    case "left_option":
        return ShortcutBinding(keyCode: 58, flags: .option, mode: config.shortcut.mode, title: "左⌥")
    case "right_shift":
        return ShortcutBinding(keyCode: 60, flags: .shift, mode: config.shortcut.mode, title: "右⇧")
    case "left_shift":
        return ShortcutBinding(keyCode: 56, flags: .shift, mode: config.shortcut.mode, title: "左⇧")
    case "right_control":
        return ShortcutBinding(keyCode: 62, flags: .control, mode: config.shortcut.mode, title: "右⌃")
    case "left_control":
        return ShortcutBinding(keyCode: 59, flags: .control, mode: config.shortcut.mode, title: "左⌃")
    default:
        return ShortcutBinding(keyCode: 54, flags: .command, mode: config.shortcut.mode, title: "右⌘")
    }
}

private func currentShortcutBinding() -> ShortcutBinding {
    makeShortcutBinding(from: loadContextConfigFromDisk())
}

private func currentContextSnapshot() -> EngineContextSnapshot {
    let maxChars = max(0, loadContextConfigFromDisk().textContext.maxChars)
    let context = focusedTextContext(maxChars: maxChars == 0 ? 256 : maxChars)
    return EngineContextSnapshot(
        frontmostBundleId: context.frontmostAppBundleId,
        textBeforeCursor: context.textBeforeCursor ?? "",
        textAfterCursor: context.textAfterCursor ?? "",
        cursorPosition: context.cursorPosition ?? 0,
        captureSource: context.captureSource,
        capturedAtMs: currentTimeMillis()
    )
}

private func runtimeTimelineContextFields(_ context: EngineContextSnapshot) -> [String: Any] {
    [
        "frontmost_bundle_id": context.frontmostBundleId ?? "",
        "capture_source": context.captureSource,
        "captured_at_ms": context.capturedAtMs,
        "text_before_chars": context.textBeforeCursor.count,
        "text_after_chars": context.textAfterCursor.count,
        "cursor_position": context.cursorPosition,
    ]
}

private func runtimeTimelineShortcutFields(_ signal: ShortcutSignal, binding: ShortcutBinding) -> [String: Any] {
    [
        "signal": {
            switch signal {
            case .press: return "press"
            case .release: return "release"
            case .doubleTap: return "double_tap"
            case .warmupHint: return "warmup_hint"
            }
        }(),
        "mode": binding.mode,
        "key_code": binding.keyCode,
        "title": binding.title,
    ]
}

private func runtimeTimelineEngineEventFields(_ event: EngineEventRecord, object: [String: Any]?) -> [String: Any] {
    var fields: [String: Any] = [
        "received_at_ms": event.receivedAtMs,
        "raw": event.raw,
    ]
    if let type = event.type {
        fields["type"] = type
    }
    if let object {
        if let sessionID = object["session_id"] {
            fields["session_id"] = sessionID
        }
        if let utteranceID = object["utterance_id"] {
            fields["utterance_id"] = utteranceID
        }
        if let name = object["name"] {
            fields["metric_name"] = name
        }
        if let reason = object["reason"] {
            fields["reason"] = reason
        }
        if let state = object["state"] {
            fields["state"] = state
        }
        if let code = object["code"] {
            fields["code"] = code
        }
        if let text = object["text"] as? String {
            for (key, value) in runtimeTimelineTextFields(text) {
                fields[key] = value
            }
        }
    }
    return fields
}


private func shortcutSignalName(_ signal: ShortcutSignal) -> String {
    switch signal {
    case .press: return "press"
    case .release: return "release"
    case .doubleTap: return "double_tap"
    case .warmupHint: return "warmup_hint"
    }
}

private func bundledSoundURL(_ fileName: String) -> URL? {
    let fileManager = FileManager.default
    let candidates: [URL?] = [
        Bundle.main.resourceURL?.appendingPathComponent("sounds/\(fileName)"),
        URL(fileURLWithPath: fileManager.currentDirectoryPath).appendingPathComponent("App/Resources/sounds/\(fileName)"),
        URL(fileURLWithPath: "/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/siri").appendingPathComponent(fileName),
    ]
    for candidate in candidates.compactMap({ $0 }) {
        if fileManager.fileExists(atPath: candidate.path) {
            return candidate
        }
    }
    return nil
}

private func playSystemFeedback(_ fileName: String) {
    guard let url = bundledSoundURL(fileName),
          let sound = NSSound(contentsOf: url, byReference: false) else {
        return
    }
    activeFeedbackSounds.removeAll { !$0.isPlaying }
    activeFeedbackSounds.append(sound)
    sound.play()
}

private final class ModifierShortcutMonitor {
    private var globalMonitor: Any?
    private var localMonitor: Any?
    private let bindingProvider: () -> ShortcutBinding
    private let handler: (ShortcutSignal) -> Void
    private var pressed = false
    private var lastTapAt: TimeInterval = 0

    init(bindingProvider: @escaping () -> ShortcutBinding, handler: @escaping (ShortcutSignal) -> Void) {
        self.bindingProvider = bindingProvider
        self.handler = handler
    }

    func start() {
        stop()
        globalMonitor = NSEvent.addGlobalMonitorForEvents(matching: .flagsChanged) { [weak self] event in
            self?.handle(event)
        }
        localMonitor = NSEvent.addLocalMonitorForEvents(matching: .flagsChanged) { [weak self] event in
            self?.handle(event)
            return event
        }
    }

    func stop() {
        if let globalMonitor {
            NSEvent.removeMonitor(globalMonitor)
            self.globalMonitor = nil
        }
        if let localMonitor {
            NSEvent.removeMonitor(localMonitor)
            self.localMonitor = nil
        }
    }

    private func handle(_ event: NSEvent) {
        let binding = bindingProvider()
        guard event.keyCode == binding.keyCode else {
            return
        }
        let isDown = event.modifierFlags.intersection(.deviceIndependentFlagsMask).contains(binding.flags)
        switch binding.mode {
        case "double_tap":
            guard isDown, !pressed else {
                if !isDown {
                    pressed = false
                }
                return
            }
            pressed = true
            let now = CFAbsoluteTimeGetCurrent()
            if now - lastTapAt <= 0.40 {
                lastTapAt = 0
                RuntimeTimeline.shared.record("user", "hotkey_signal", fields: runtimeTimelineShortcutFields(.doubleTap, binding: binding))
                handler(.doubleTap)
            } else {
                lastTapAt = now
                RuntimeTimeline.shared.record("user", "hotkey_signal", fields: runtimeTimelineShortcutFields(.warmupHint, binding: binding))
                handler(.warmupHint)
            }
        default:
            if isDown && !pressed {
                pressed = true
                RuntimeTimeline.shared.record("user", "hotkey_signal", fields: runtimeTimelineShortcutFields(.press, binding: binding))
                handler(.press)
            } else if !isDown && pressed {
                pressed = false
                RuntimeTimeline.shared.record("user", "hotkey_signal", fields: runtimeTimelineShortcutFields(.release, binding: binding))
                handler(.release)
            }
        }
    }
}

private struct EngineRuntimeSnapshot {
    var helperState = "starting"
    var authState = "cold"
    var authSource = "—"
    var isRecording = false
    var lastPartial = ""
    var lastFinal = ""
    var lastError = ""
    var warmed = false
    var audioLevel: Double = 0
    var shortcutTitle = "右⌘ / hold"
}

private final class AppEngineController {
    var onSnapshot: ((EngineRuntimeSnapshot) -> Void)?
    var onPartialText: ((String) -> Void)?
    var onFinalText: ((String) -> Void)?
    var onAudioLevel: ((Double) -> Void)?
    var onRecordingStarted: (() -> Void)?
    var onRecordingStopped: ((String) -> Void)?

    private let launcher: EngineLaunchInfo
    private let currentDirectoryURL: URL
    private let queue = DispatchQueue(label: "shuo.app.engine")
    private var bridge: EngineProcessBridge?
    private var manualShutdown = false
    private var snapshot = EngineRuntimeSnapshot()
    private var activeSessionID: String?

    init(options: AppOptions) {
        let currentDirectoryURL = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
        self.currentDirectoryURL = currentDirectoryURL
        self.launcher = resolveEngineLauncher(
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
    }

    func start() {
        queue.async { self.startLocked() }
    }

    func warmup(with context: EngineContextSnapshot, force: Bool = false) {
        queue.async {
            guard let bridge = self.bridge, bridge.isRunning else { return }
            var fields = runtimeTimelineContextFields(context)
            fields["force"] = force
            fields["active_session_id"] = self.activeSessionID ?? ""
            RuntimeTimeline.shared.record("host", "warmup_requested", fields: fields)
            try? bridge.send(.updateContext(context))
            try? bridge.send(.warmup(force: force))
        }
    }

    func startRecording(with context: EngineContextSnapshot, trigger: String) {
        queue.async {
            guard let bridge = self.bridge, bridge.isRunning else { return }
            let sessionId = "shuo-\(UUID().uuidString.lowercased())"
            self.activeSessionID = sessionId
            var fields = runtimeTimelineContextFields(context)
            fields["session_id"] = sessionId
            fields["trigger"] = trigger
            RuntimeTimeline.shared.record("host", "start_recording_requested", fields: fields)
            try? bridge.send(.updateContext(context))
            try? bridge.send(.startRecording(
                sessionId: sessionId,
                trigger: trigger,
                contextSnapshot: context
            ))
        }
    }

    func stopRecording(cancel: Bool) {
        queue.async {
            guard let bridge = self.bridge, bridge.isRunning else { return }
            RuntimeTimeline.shared.record("host", cancel ? "cancel_recording_requested" : "stop_recording_requested", fields: [
                "session_id": self.activeSessionID ?? "",
            ])
            try? bridge.send(cancel ? .cancelRecording() : .stopRecording())
        }
    }

    func refreshAuth() {
        queue.async {
            guard let bridge = self.bridge, bridge.isRunning else { return }
            RuntimeTimeline.shared.record("host", "refresh_auth_requested")
            try? bridge.send(.refreshAuth())
        }
    }

    func exportDiagnostics() {
        queue.async {
            guard let bridge = self.bridge, bridge.isRunning else { return }
            RuntimeTimeline.shared.record("host", "export_diagnostics_requested", fields: [
                "session_id": self.activeSessionID ?? "",
            ])
            try? bridge.send(.exportDiagnostics())
        }
    }

    func shutdown() {
        queue.async {
            self.manualShutdown = true
            guard let bridge = self.bridge else { return }
            RuntimeTimeline.shared.record("host", "shutdown_requested")
            try? bridge.send(.shutdown())
            _ = bridge.waitForExit(timeout: 2.0)
            self.bridge = nil
        }
    }

    func restart() {
        queue.async {
            self.manualShutdown = true
            RuntimeTimeline.shared.record("host", "restart_requested", fields: [
                "session_id": self.activeSessionID ?? "",
            ])
            if let bridge = self.bridge {
                try? bridge.send(.shutdown())
                _ = bridge.waitForExit(timeout: 2.0)
            }
            self.bridge = nil
            self.manualShutdown = false
            self.startLocked()
        }
    }

    private func startLocked() {
        guard bridge == nil else { return }
        snapshot.helperState = "starting"
        emit()
        RuntimeTimeline.shared.record("host", "engine_starting", fields: [
            "executable": launcher.executable,
            "arguments": launcher.arguments,
        ])

        let bridge = EngineProcessBridge(launcher: launcher, currentDirectoryURL: currentDirectoryURL)
        bridge.onEvent = { [weak self] event in
            self?.route(event: event)
        }
        bridge.onTermination = { [weak self] status in
            self?.queue.async { self?.handleTermination(status: status) }
        }
        bridge.onStderrLine = { [weak self] line in
            self?.queue.async {
                self?.snapshot.lastError = line
                self?.emit()
            }
        }

        do {
            self.bridge = bridge
            try bridge.start()
            var cursor = 0
            guard bridge.waitForEvent(after: &cursor, matching: ["ready"], timeout: 8.0) != nil else {
                throw CLIError.timeout("engine ready timeout")
            }
            try bridge.send(.hello())
            guard bridge.waitForEvent(after: &cursor, matching: ["ready"], timeout: 8.0) != nil else {
                throw CLIError.timeout("engine hello ack timeout")
            }
            snapshot.helperState = "ready"
            snapshot.lastError = ""
            emit()
        } catch {
            snapshot.helperState = "failed"
            snapshot.lastError = "\(error)"
            emit()
            self.bridge = nil
            scheduleRestart()
        }
    }

    private func route(event: EngineEventRecord) {
        let object = event.jsonObject
        RuntimeTimeline.shared.record("host", "engine_event_routed", fields: runtimeTimelineEngineEventFields(event, object: object))
        let dispatchedRealtime = dispatchRealtimeCallback(for: event, object: object)
        queue.async { [weak self] in
            self?.handle(event: event, object: object, skipRealtimeCallback: dispatchedRealtime)
        }
    }

    private func dispatchRealtimeCallback(for event: EngineEventRecord, object: [String: Any]?) -> Bool {
        switch event.type {
        case "recording_started":
            DispatchQueue.main.async { [onRecordingStarted] in
                onRecordingStarted?()
            }
            return true
        case "recording_stopped":
            let reason = object?["reason"] as? String ?? "stopped"
            DispatchQueue.main.async { [onRecordingStopped] in
                onRecordingStopped?(reason)
            }
            return true
        case "audio_level":
            let peak = object?["level_peak"] as? Double ?? 0
            let rms = object?["level_rms"] as? Double ?? 0
            let level = max(peak, rms * 4)
            DispatchQueue.main.async { [onAudioLevel] in
                onAudioLevel?(level)
            }
            return true
        case "partial":
            let text = object?["text"] as? String ?? ""
            DispatchQueue.main.async { [onPartialText] in
                onPartialText?(text)
            }
            return true
        case "final":
            let text = object?["text"] as? String ?? ""
            DispatchQueue.main.async { [onFinalText] in
                onFinalText?(text)
            }
            return true
        default:
            return false
        }
    }

    private func handle(event: EngineEventRecord, object: [String: Any]?, skipRealtimeCallback: Bool) {
        var shouldEmitSnapshot = true
        switch event.type {
        case "ready":
            snapshot.helperState = snapshot.isRecording ? "recording" : "ready"
        case "recording_started":
            activeSessionID = object?["session_id"] as? String ?? activeSessionID
            snapshot.isRecording = true
            snapshot.helperState = "recording"
            snapshot.lastError = ""
            if !skipRealtimeCallback {
                DispatchQueue.main.async { [onRecordingStarted] in
                    onRecordingStarted?()
                }
            }
        case "recording_stopped":
            snapshot.isRecording = false
            snapshot.helperState = "ready"
            let reason = object?["reason"] as? String ?? "stopped"
            if reason != "flush_pending" {
                activeSessionID = nil
            }
            if !skipRealtimeCallback {
                DispatchQueue.main.async { [onRecordingStopped] in
                    onRecordingStopped?(reason)
                }
            }
        case "audio_level":
            let peak = object?["level_peak"] as? Double ?? 0
            let rms = object?["level_rms"] as? Double ?? 0
            let level = max(peak, rms * 4)
            snapshot.audioLevel = level
            shouldEmitSnapshot = false
            if !skipRealtimeCallback {
                DispatchQueue.main.async { [onAudioLevel] in
                    onAudioLevel?(level)
                }
            }
        case "partial":
            let text = object?["text"] as? String ?? ""
            snapshot.lastPartial = text
            shouldEmitSnapshot = false
            if !skipRealtimeCallback {
                DispatchQueue.main.async { [onPartialText] in
                    onPartialText?(text)
                }
            }
        case "final":
            let text = object?["text"] as? String ?? ""
            activeSessionID = nil
            snapshot.isRecording = false
            snapshot.helperState = "ready"
            snapshot.lastPartial = ""
            snapshot.lastFinal = text
            if !skipRealtimeCallback {
                DispatchQueue.main.async { [onFinalText] in
                    onFinalText?(text)
                }
            }
        case "auth_state":
            snapshot.authState = object?["state"] as? String ?? snapshot.authState
            snapshot.authSource = object?["source"] as? String ?? snapshot.authSource
        case "metrics":
            let name = object?["name"] as? String ?? ""
            if name == "warmed" || name == "transport_ready" {
                snapshot.warmed = true
                if !snapshot.isRecording {
                    snapshot.helperState = "ready"
                }
            }
        case "error":
            snapshot.lastError = object?["message"] as? String ?? event.raw
            if !snapshot.isRecording {
                snapshot.helperState = "degraded"
            }
        case "fatal":
            activeSessionID = nil
            snapshot.isRecording = false
            snapshot.helperState = "failed"
            snapshot.lastError = object?["message"] as? String ?? event.raw
        default:
            break
        }
        if shouldEmitSnapshot {
            emit()
        }
    }

    private func handleTermination(status: Int32) {
        bridge = nil
        activeSessionID = nil
        snapshot.isRecording = false
        if manualShutdown {
            snapshot.helperState = "stopped"
        } else {
            snapshot.helperState = status == 0 ? "stopped" : "failed"
            if status != 0 {
                snapshot.lastError = "helper exited with status \(status)"
            }
            scheduleRestart()
        }
        emit()
    }

    private func scheduleRestart() {
        queue.asyncAfter(deadline: .now() + 1.0) { [weak self] in
            guard let self, !self.manualShutdown, self.bridge == nil else { return }
            self.startLocked()
        }
    }

    private func emit() {
        snapshot.shortcutTitle = shortcutMenuTitle()
        let current = snapshot
        DispatchQueue.main.async { [onSnapshot] in
            onSnapshot?(current)
        }
    }

    private func shortcutMenuTitle() -> String {
        let binding = currentShortcutBinding()
        if binding.mode == "double_tap" {
            return "双击\(binding.title)切换"
        }
        return "按住\(binding.title)说话"
    }
}

private func shortcutMenuTitle() -> String {
    let binding = currentShortcutBinding()
    if binding.mode == "double_tap" {
        return "双击\(binding.title)切换"
    }
    return "按住\(binding.title)说话"
}

private final class ShuoAppDelegate: NSObject, NSApplicationDelegate {
    private let options: AppOptions
    private let engine: AppEngineController
    private let transcriptHistory = TranscriptHistoryStore()
    private let statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    private let overlay = SubtitleOverlayController()
    private var settingsWindows: [NSWindow] = []
    private var transcriptHistoryWindow: NSWindow?
    private var hotkeyMonitor: ModifierShortcutMonitor?
    private var didRequestInitialWarmup = false
    private var injectionTarget: FocusedInjectionTarget?
    private var latestSnapshot = EngineRuntimeSnapshot()
    private var permissionPollTimer: Timer?

    private let stateItem = NSMenuItem(title: "状态：启动中", action: nil, keyEquivalent: "")
    private let transcriptMenuItem = NSMenuItem(title: "最近转录", action: nil, keyEquivalent: "")
    private lazy var permissionItem = NSMenuItem(
        title: "辅助功能：检查中",
        action: #selector(requestAccessibilityPermission),
        keyEquivalent: ""
    )
    private lazy var settingsItem = NSMenuItem(title: "设置…", action: #selector(openSettings), keyEquivalent: ",")
    private lazy var quitItem = NSMenuItem(title: "退出", action: #selector(quit), keyEquivalent: "q")

    init(options: AppOptions) {
        self.options = options
        self.engine = AppEngineController(options: options)
        super.init()
        engine.onSnapshot = { [weak self] snapshot in
            self?.render(snapshot: snapshot)
        }
        engine.onRecordingStarted = { [weak self] in
            RuntimeTimeline.shared.record("ui", "recording_started_callback")
            playSystemFeedback(systemSfxBegin)
            self?.overlay.showWaveformOnly()
        }
        engine.onRecordingStopped = { [weak self] reason in
            guard let self else { return }
            RuntimeTimeline.shared.record("ui", "recording_stopped_callback", fields: [
                "reason": reason,
            ])
            if reason != "cancelled" {
                playSystemFeedback(systemSfxConfirm)
            }
            if reason != "flush_pending" {
                self.overlay.hide()
                self.injectionTarget = nil
            }
            self.rewarmIfNeeded()
        }
        engine.onAudioLevel = { [weak self] level in
            self?.overlay.updateLevel(level)
        }
        engine.onPartialText = { [weak self] text in
            RuntimeTimeline.shared.record("ui", "partial_callback", fields: runtimeTimelineTextFields(text))
            self?.overlay.showPartial(text)
        }
        engine.onFinalText = { [weak self] text in
            guard let self else { return }
            RuntimeTimeline.shared.record("ui", "final_callback", fields: runtimeTimelineTextFields(text))
            let commitText = self.overlay.showFinal(text)
            let resolvedText = commitText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? text : commitText
            var injectionSucceeded = false
            if !resolvedText.isEmpty && self.options.injectFinalText {
                injectionSucceeded = insertTextIntoFocusedElement(resolvedText, target: self.injectionTarget)
                RuntimeTimeline.shared.record("ui", "final_injection", fields: [
                    "succeeded": injectionSucceeded,
                    "target_pid": self.injectionTarget?.processIdentifier ?? -1,
                    "inject_enabled": self.options.injectFinalText,
                    "text": runtimeTimelineTextFields(resolvedText),
                ])
            }
            self.transcriptHistory.add(resolvedText)
            RuntimeTimeline.shared.record("ui", "transcript_saved", fields: runtimeTimelineTextFields(resolvedText))
            self.rebuildTranscriptMenu()
            self.injectionTarget = nil
            self.rewarmIfNeeded()
        }
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        RuntimeTimeline.shared.record("app", "launched", fields: [
            "transport": options.transport,
            "hotkey_enabled": options.enableHotkey,
            "inject_final_text": options.injectFinalText,
            "timeline_file": RuntimeTimeline.shared.currentFilePath(),
        ])
        NSApplication.shared.setActivationPolicy(.accessory)
        installMenu()
        render(snapshot: EngineRuntimeSnapshot())
        if options.enableHotkey {
            hotkeyMonitor = ModifierShortcutMonitor(
                bindingProvider: currentShortcutBinding,
                handler: { [weak self] signal in
                    DispatchQueue.main.async { self?.handleShortcut(signal) }
                }
            )
            hotkeyMonitor?.start()
        }
        if !accessibilityTrusted() {
            RuntimeTimeline.shared.record("permission", "accessibility_prompt_requested_on_launch")
            _ = promptForAccessibilityPermission()
            startPermissionPolling()
        }
        engine.start()
    }

    func applicationWillTerminate(_ notification: Notification) {
        RuntimeTimeline.shared.record("app", "will_terminate")
        hotkeyMonitor?.stop()
        stopPermissionPolling()
        engine.shutdown()
    }

    private func installMenu() {
        setStatusIcon(recording: false)
        permissionItem.target = self
        transcriptMenuItem.submenu = NSMenu(title: "最近转录")

        let menu = NSMenu()
        stateItem.isEnabled = false
        menu.addItem(stateItem)
        menu.addItem(transcriptMenuItem)
        menu.addItem(permissionItem)
        menu.addItem(.separator())
        menu.addItem(settingsItem)
        menu.addItem(quitItem)
        statusItem.menu = menu
        rebuildTranscriptMenu()
    }

    private func render(snapshot: EngineRuntimeSnapshot) {
        latestSnapshot = snapshot
        let trusted = accessibilityTrusted()
        setStatusIcon(recording: snapshot.isRecording)
        stateItem.title = "状态：\(stateLabel(for: snapshot))"
        permissionItem.title = trusted ? "辅助功能：已授权" : "辅助功能：未授权（点此授权）"
        permissionItem.isEnabled = !trusted
        permissionItem.isHidden = trusted
        if trusted {
            stopPermissionPolling()
        } else {
            startPermissionPolling()
        }
        if snapshot.helperState != "ready" {
            didRequestInitialWarmup = false
        }
        if snapshot.helperState == "ready" && !didRequestInitialWarmup {
            didRequestInitialWarmup = true
            engine.warmup(with: currentContextSnapshot())
        }
    }

    private func stateLabel(for snapshot: EngineRuntimeSnapshot) -> String {
        if snapshot.isRecording {
            return "录音中"
        }
        switch snapshot.helperState {
        case "ready": return "就绪"
        case "starting": return "启动中"
        case "failed": return "失败"
        case "degraded": return "降级"
        case "stopped": return "已停止"
        default: return snapshot.helperState
        }
    }

    private func rebuildTranscriptMenu() {
        guard let submenu = transcriptMenuItem.submenu else { return }
        submenu.removeAllItems()

        let searchItem = NSMenuItem(title: "搜索最近转录…", action: #selector(openTranscriptHistorySearch), keyEquivalent: "")
        searchItem.target = self
        submenu.addItem(searchItem)
        submenu.addItem(.separator())

        if transcriptHistory.entries.isEmpty {
            let emptyItem = NSMenuItem(title: "暂无转录", action: nil, keyEquivalent: "")
            emptyItem.isEnabled = false
            submenu.addItem(emptyItem)
            return
        }

        for entry in transcriptHistory.entries {
            let item = NSMenuItem(
                title: transcriptHistoryMenuTitle(for: entry.text),
                action: #selector(copyTranscriptHistoryItem(_:)),
                keyEquivalent: ""
            )
            item.target = self
            item.toolTip = entry.text
            item.representedObject = entry.text
            submenu.addItem(item)
        }
    }

    private func handleShortcut(_ signal: ShortcutSignal) {
        let binding = currentShortcutBinding()
        RuntimeTimeline.shared.record("user", "shortcut_\(shortcutSignalName(signal))", fields: [
            "binding": binding.title,
            "mode": binding.mode,
        ])
        switch signal {
        case .press:
            startRecording()
        case .release:
            stopRecording()
        case .doubleTap:
            if latestSnapshot.isRecording {
                stopRecording()
            } else {
                startRecording()
            }
        case .warmupHint:
            warmup()
        }
    }

    private func rewarmIfNeeded() {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { [weak self] in
            RuntimeTimeline.shared.record("host", "rewarm_scheduled")
            self?.engine.warmup(with: currentContextSnapshot())
        }
    }

    @objc private func startRecording() {
        let context = currentContextSnapshot()
        injectionTarget = captureFocusedInjectionTarget()
        var fields = runtimeTimelineContextFields(context)
        fields["target_pid"] = injectionTarget?.processIdentifier ?? -1
        fields["trigger"] = "swift_menu_bar"
        RuntimeTimeline.shared.record("user", "start_recording", fields: fields)
        overlay.resetSession()
        overlay.showWaveformOnly()
        engine.startRecording(with: context, trigger: "swift_menu_bar")
    }

    @objc private func stopRecording() {
        RuntimeTimeline.shared.record("user", "stop_recording", fields: [
            "cancel": false,
        ])
        engine.stopRecording(cancel: false)
    }

    @objc private func refreshAuth() {
        RuntimeTimeline.shared.record("user", "refresh_auth")
        engine.refreshAuth()
    }

    @objc private func warmup() {
        let context = currentContextSnapshot()
        RuntimeTimeline.shared.record("user", "warmup", fields: runtimeTimelineContextFields(context))
        engine.warmup(with: context)
    }

    @objc private func restartHelper() {
        RuntimeTimeline.shared.record("user", "restart_helper")
        didRequestInitialWarmup = false
        engine.restart()
    }

    @objc private func openSettings() {
        let window = makeSettingsWindow()
        settingsWindows.append(window)
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    @objc private func requestAccessibilityPermission() {
        RuntimeTimeline.shared.record("user", "request_accessibility_permission")
        NSApp.activate(ignoringOtherApps: true)
        let trusted = promptForAccessibilityPermission()
        RuntimeTimeline.shared.record("permission", "accessibility_prompt_result", fields: [
            "trusted": trusted,
        ])
        if !trusted {
            let opened = openAccessibilitySettings()
            RuntimeTimeline.shared.record("permission", "open_accessibility_settings", fields: [
                "opened": opened,
            ])
            startPermissionPolling()
        } else {
            render(snapshot: latestSnapshot)
        }
    }

    @objc private func openTranscriptHistorySearch() {
        if let transcriptHistoryWindow {
            transcriptHistoryWindow.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            return
        }
        let window = makeTranscriptHistoryWindow(store: transcriptHistory)
        transcriptHistoryWindow = window
        NSApp.activate(ignoringOtherApps: true)
    }

    @objc private func copyTranscriptHistoryItem(_ sender: NSMenuItem) {
        guard let text = sender.representedObject as? String else { return }
        transcriptHistory.copy(text)
    }

    @objc private func quit() {
        NSApp.terminate(nil)
    }

    private func setStatusIcon(recording: Bool) {
        guard let button = statusItem.button else { return }
        if let image = NSImage(
            systemSymbolName: recording ? "mic.fill" : "mic",
            accessibilityDescription: "Shuo"
        ) {
            image.isTemplate = true
            button.image = image
            button.title = ""
        } else {
            button.image = nil
            button.title = recording ? "●" : "🎤"
        }
    }

    private func startPermissionPolling() {
        guard permissionPollTimer == nil else { return }
        RuntimeTimeline.shared.record("permission", "accessibility_polling_started")
        let timer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] timer in
            guard let self else {
                timer.invalidate()
                return
            }
            if accessibilityTrusted() {
                RuntimeTimeline.shared.record("permission", "accessibility_polling_succeeded")
                self.stopPermissionPolling()
                self.render(snapshot: self.latestSnapshot)
            }
        }
        permissionPollTimer = timer
        RunLoop.main.add(timer, forMode: .common)
    }

    private func stopPermissionPolling() {
        if permissionPollTimer != nil {
            RuntimeTimeline.shared.record("permission", "accessibility_polling_stopped")
        }
        permissionPollTimer?.invalidate()
        permissionPollTimer = nil
    }
}

func runMenuBarApp(options: AppOptions) {
    let app = NSApplication.shared
    let delegate = ShuoAppDelegate(options: options)
    app.delegate = delegate
    app.setActivationPolicy(.accessory)
    app.run()
}

private extension String {
    func ifEmpty(_ fallback: String) -> String {
        isEmpty ? fallback : self
    }
}
