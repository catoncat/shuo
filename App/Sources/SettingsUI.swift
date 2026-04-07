import AppKit
import Foundation
import SwiftUI

private struct SettingsOption: Identifiable {
    let label: String
    let value: String
    var id: String { value }
}

private struct InfoPopover: View {
    let text: String
    @State private var isPresented = false

    var body: some View {
        Button {
            isPresented.toggle()
        } label: {
            Image(systemName: "questionmark.circle")
                .foregroundStyle(.secondary)
        }
        .buttonStyle(.plain)
        .help("点击查看说明")
        .popover(isPresented: $isPresented, arrowEdge: .bottom) {
            Text(text)
                .font(.callout)
                .padding(12)
                .frame(width: 280, alignment: .leading)
        }
    }
}

private let shortcutKeyOptions: [SettingsOption] = [
    .init(label: "右侧 ⌘ Command", value: "right_command"),
    .init(label: "左侧 ⌘ Command", value: "left_command"),
    .init(label: "右侧 ⌥ Option", value: "right_option"),
    .init(label: "左侧 ⌥ Option", value: "left_option"),
    .init(label: "右侧 ⇧ Shift", value: "right_shift"),
    .init(label: "左侧 ⇧ Shift", value: "left_shift"),
    .init(label: "右侧 ⌃ Control", value: "right_control"),
    .init(label: "左侧 ⌃ Control", value: "left_control"),
]

private let shortcutModeOptions: [SettingsOption] = [
    .init(label: "按住说话", value: "hold"),
    .init(label: "双击切换", value: "double_tap"),
]

private let textModeOptions: [SettingsOption] = [
    .init(label: "自动", value: "auto"),
    .init(label: "关闭", value: "off"),
    .init(label: "静态", value: "static"),
]

struct ContextConfig: Codable {
    var version: Int = 1
    var recognition: RecognitionConfig = .init()
    var hotwords: [String] = []
    var userTerms: [String] = []
    var textContext: TextContextConfig = .init()
    var imeContext: ImeContextConfig = .init()
    var advanced: AdvancedConfig = .init()
    var shortcut: ShortcutConfig = .init()

    enum CodingKeys: String, CodingKey {
        case version
        case recognition
        case hotwords
        case userTerms = "user_terms"
        case textContext = "text_context"
        case imeContext = "ime_context"
        case advanced
        case shortcut
    }

    init() {}

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        version = try c.decodeIfPresent(Int.self, forKey: .version) ?? 1
        recognition = try c.decodeIfPresent(RecognitionConfig.self, forKey: .recognition) ?? .init()
        hotwords = try c.decodeIfPresent([String].self, forKey: .hotwords) ?? []
        userTerms = try c.decodeIfPresent([String].self, forKey: .userTerms) ?? []
        textContext = try c.decodeIfPresent(TextContextConfig.self, forKey: .textContext) ?? .init()
        imeContext = try c.decodeIfPresent(ImeContextConfig.self, forKey: .imeContext) ?? .init()
        advanced = try c.decodeIfPresent(AdvancedConfig.self, forKey: .advanced) ?? .init()
        shortcut = try c.decodeIfPresent(ShortcutConfig.self, forKey: .shortcut) ?? .init()
    }
}

struct ShortcutConfig: Codable {
    var key: String = "right_command"
    var mode: String = "hold"

    init() {}

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        key = try c.decodeIfPresent(String.self, forKey: .key) ?? "right_command"
        mode = try c.decodeIfPresent(String.self, forKey: .mode) ?? "hold"
    }
}

struct RecognitionConfig: Codable {
    var enablePunctuation: Bool = true
    var enableSpeechRejection: Bool = false

    enum CodingKeys: String, CodingKey {
        case enablePunctuation = "enable_punctuation"
        case enableSpeechRejection = "enable_speech_rejection"
    }

    init() {}

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        enablePunctuation = try c.decodeIfPresent(Bool.self, forKey: .enablePunctuation) ?? true
        enableSpeechRejection = try c.decodeIfPresent(Bool.self, forKey: .enableSpeechRejection) ?? false
    }
}

struct TextContextConfig: Codable {
    var mode: String = "auto"
    var maxChars: Int = 256
    var text: String = ""
    var cursorPosition: Int = 0

    enum CodingKeys: String, CodingKey {
        case mode
        case maxChars = "max_chars"
        case text
        case cursorPosition = "cursor_position"
    }

    init() {}

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        mode = try c.decodeIfPresent(String.self, forKey: .mode) ?? "auto"
        maxChars = try c.decodeIfPresent(Int.self, forKey: .maxChars) ?? 256
        text = try c.decodeIfPresent(String.self, forKey: .text) ?? ""
        cursorPosition = try c.decodeIfPresent(Int.self, forKey: .cursorPosition) ?? 0
    }
}

struct ImeContextConfig: Codable {
    var inputType: String = "default"

    enum CodingKeys: String, CodingKey {
        case inputType = "input_type"
    }

    init() {}

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        inputType = try c.decodeIfPresent(String.self, forKey: .inputType) ?? "default"
    }
}

struct AdvancedConfig: Codable {
    var useUserDictionary: Bool = true
    var enableTextFilter: Bool = true
    var enableAsrTwopass: Bool = true
    var enableAsrThreepass: Bool = true
    var removeSpaceBetweenHanEng: Bool = false
    var removeSpaceBetweenHanNum: Bool = false
    var strongDdc: Bool = false

    enum CodingKeys: String, CodingKey {
        case useUserDictionary = "use_user_dictionary"
        case enableTextFilter = "enable_text_filter"
        case enableAsrTwopass = "enable_asr_twopass"
        case enableAsrThreepass = "enable_asr_threepass"
        case removeSpaceBetweenHanEng = "remove_space_between_han_eng"
        case removeSpaceBetweenHanNum = "remove_space_between_han_num"
        case strongDdc = "strong_ddc"
    }

    init() {}

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        useUserDictionary = try c.decodeIfPresent(Bool.self, forKey: .useUserDictionary) ?? true
        enableTextFilter = try c.decodeIfPresent(Bool.self, forKey: .enableTextFilter) ?? true
        enableAsrTwopass = try c.decodeIfPresent(Bool.self, forKey: .enableAsrTwopass) ?? true
        enableAsrThreepass = try c.decodeIfPresent(Bool.self, forKey: .enableAsrThreepass) ?? true
        removeSpaceBetweenHanEng = try c.decodeIfPresent(Bool.self, forKey: .removeSpaceBetweenHanEng) ?? false
        removeSpaceBetweenHanNum = try c.decodeIfPresent(Bool.self, forKey: .removeSpaceBetweenHanNum) ?? false
        strongDdc = try c.decodeIfPresent(Bool.self, forKey: .strongDdc) ?? false
    }
}

func resolveContextConfigPath() -> URL? {
    if let raw = ProcessInfo.processInfo.environment["CONTEXT_CONFIG_PATH"], !raw.isEmpty {
        return URL(fileURLWithPath: raw)
    }

    if let resourceURL = Bundle.main.resourceURL {
        for name in ["configs/shuo.context.json", "configs/hj_dictation.context.json"] {
            let bundledCandidate = resourceURL.appendingPathComponent(name)
            if FileManager.default.fileExists(atPath: bundledCandidate.path) {
                return bundledCandidate
            }
        }
    }

    var roots: [URL] = []
    if let exe = Bundle.main.executableURL?.deletingLastPathComponent() {
        roots.append(exe)
    }
    roots.append(URL(fileURLWithPath: FileManager.default.currentDirectoryPath))

    for root in roots {
        var current: URL? = root
        for _ in 0..<8 {
            guard let dir = current else { break }
            for name in ["configs/shuo.context.json", "configs/hj_dictation.context.json"] {
                let candidate = dir.appendingPathComponent(name)
                if FileManager.default.fileExists(atPath: candidate.path) {
                    return candidate
                }
            }
            current = dir.deletingLastPathComponent()
            if current == dir { break }
        }
    }
    return nil
}

private final class ContextConfigCacheStore {
    static let shared = ContextConfigCacheStore()

    private let queue = DispatchQueue(label: "shuo.app.context-config-cache", attributes: .concurrent)
    private var cached = ContextConfig()
    private var loaded = false

    func snapshot() -> ContextConfig {
        let cachedConfig: ContextConfig? = queue.sync {
            self.loaded ? self.cached : nil
        }
        if let cachedConfig {
            return cachedConfig
        }
        return reload()
    }

    @discardableResult
    func reload() -> ContextConfig {
        let config = loadContextConfigFromDiskUncached()
        queue.sync(flags: .barrier) {
            self.cached = config
            self.loaded = true
        }
        return config
    }

    func update(_ config: ContextConfig) {
        queue.sync(flags: .barrier) {
            self.cached = config
            self.loaded = true
        }
    }
}

private func loadContextConfigFromDiskUncached() -> ContextConfig {
    guard let path = resolveContextConfigPath(),
          let data = try? Data(contentsOf: path),
          let config = try? JSONDecoder().decode(ContextConfig.self, from: data) else {
        return .init()
    }
    return config
}

func loadContextConfigFromDisk() -> ContextConfig {
    ContextConfigCacheStore.shared.snapshot()
}

@discardableResult
func reloadContextConfigFromDisk() -> ContextConfig {
    ContextConfigCacheStore.shared.reload()
}

func updateContextConfigInMemory(_ config: ContextConfig) {
    ContextConfigCacheStore.shared.update(config)
}

private final class SettingsStore: ObservableObject {
    enum StatusLevel {
        case info
        case success
        case error
    }

    @Published var config: ContextConfig = .init()
    @Published var status: String = ""
    @Published var statusLevel: StatusLevel = .info
    @Published var lastSavedAt: String = "—"

    let configPath: URL?

    init() {
        configPath = resolveContextConfigPath()
        load()
    }

    func load() {
        guard configPath != nil else {
            status = "未找到配置路径（CONTEXT_CONFIG_PATH 或 configs/shuo.context.json）"
            statusLevel = .error
            return
        }
        config = reloadContextConfigFromDisk()
        status = "已加载"
        statusLevel = .info
    }

    func save() {
        guard let path = configPath else {
            status = "保存失败：没有可用配置路径"
            statusLevel = .error
            return
        }
        do {
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            let data = try encoder.encode(config)
            try FileManager.default.createDirectory(at: path.deletingLastPathComponent(), withIntermediateDirectories: true)
            try data.write(to: path, options: [.atomic])
            updateContextConfigInMemory(config)
            let formatter = DateFormatter()
            formatter.locale = Locale(identifier: "zh_CN")
            formatter.dateFormat = "HH:mm:ss"
            lastSavedAt = formatter.string(from: Date())
            status = "已保存并生效"
            statusLevel = .success
        } catch {
            status = "保存失败：\(error.localizedDescription)"
            statusLevel = .error
        }
    }
}

private struct TriggerInteractionSettingsView: View {
    @Binding var config: ContextConfig

    var body: some View {
        Form {
            Section("快捷键设置") {
                Picker("触发按键", selection: $config.shortcut.key) {
                    ForEach(shortcutKeyOptions) { opt in
                        Text(opt.label).tag(opt.value)
                    }
                }

                Picker("触发模式", selection: $config.shortcut.mode) {
                    ForEach(shortcutModeOptions) { opt in
                        Text(opt.label).tag(opt.value)
                    }
                }
            }
        }
        .formStyle(.grouped)
        .padding(16)
    }
}

private struct RecognitionTextSettingsView: View {
    @Binding var config: ContextConfig

    private var addSpaceBetweenHanEng: Binding<Bool> {
        Binding(
            get: { !config.advanced.removeSpaceBetweenHanEng },
            set: { config.advanced.removeSpaceBetweenHanEng = !$0 }
        )
    }

    private var addSpaceBetweenHanNum: Binding<Bool> {
        Binding(
            get: { !config.advanced.removeSpaceBetweenHanNum },
            set: { config.advanced.removeSpaceBetweenHanNum = !$0 }
        )
    }

    private var hotwordsText: Binding<String> {
        Binding(
            get: { config.hotwords.joined(separator: ", ") },
            set: { config.hotwords = $0.split(separator: ",").map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty } }
        )
    }

    private var userTermsText: Binding<String> {
        Binding(
            get: { config.userTerms.joined(separator: ", ") },
            set: { config.userTerms = $0.split(separator: ",").map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty } }
        )
    }

    var body: some View {
        Form {
            Section("输入质量") {
                Toggle("自动标点", isOn: $config.recognition.enablePunctuation)
                Toggle("非语音片段抑制", isOn: $config.recognition.enableSpeechRejection)
            }

            Section("词条与上下文") {
                VStack(alignment: .leading, spacing: 6) {
                    Text("热词（每行一个，或用逗号分隔）")
                        .font(.subheadline)
                    TextEditor(text: hotwordsText)
                        .font(.body)
                        .frame(minHeight: 72)
                        .padding(4)
                        .overlay(
                            RoundedRectangle(cornerRadius: 8)
                                .stroke(Color.secondary.opacity(0.3), lineWidth: 1)
                        )
                }

                VStack(alignment: .leading, spacing: 6) {
                    Text("用户词条（每行一个，或用逗号分隔）")
                        .font(.subheadline)
                    TextEditor(text: userTermsText)
                        .font(.body)
                        .frame(minHeight: 72)
                        .padding(4)
                        .overlay(
                            RoundedRectangle(cornerRadius: 8)
                                .stroke(Color.secondary.opacity(0.3), lineWidth: 1)
                        )
                }

                Toggle("启用用户词典", isOn: $config.advanced.useUserDictionary)

                Picker("文本上下文模式", selection: $config.textContext.mode) {
                    ForEach(textModeOptions) { opt in
                        Text(opt.label).tag(opt.value)
                    }
                }

                HStack {
                    HStack(spacing: 6) {
                        Text("最大上下文字符数")
                        InfoPopover(text: "建议范围 256～2048。数值越大，越依赖上下文，但处理开销也会增加。")
                    }
                    Spacer()
                    TextField("", value: $config.textContext.maxChars, format: .number)
                        .multilineTextAlignment(.trailing)
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 140)
                }
            }

            Section("识别优化") {
                Toggle(isOn: $config.advanced.enableAsrTwopass) {
                    HStack(spacing: 6) {
                        Text("双阶段识别（ASR）")
                        InfoPopover(text: "先给出快速结果，再做一次纠错重算，通常更准。")
                    }
                }
                Toggle(isOn: $config.advanced.enableAsrThreepass) {
                    HStack(spacing: 6) {
                        Text("三阶段识别（ASR）")
                        InfoPopover(text: "在双阶段基础上再加一轮优化，精度更高但延迟略增。")
                    }
                }
                Toggle("中文与英文自动加空格", isOn: addSpaceBetweenHanEng)
                Toggle("中文与数字自动加空格", isOn: addSpaceBetweenHanNum)
                Toggle(isOn: $config.advanced.strongDdc) {
                    HStack(spacing: 6) {
                        Text("强纠错（DDC）")
                        InfoPopover(text: "更激进的纠错策略，能修正更多错词，但有时会过度改写。")
                    }
                }
            }

            Section("输出整理") {
                Toggle(isOn: $config.advanced.enableTextFilter) {
                    HStack(spacing: 6) {
                        Text("启用文本过滤（去噪清理）")
                        InfoPopover(text: "用于清理明显噪声字符、异常重复符号等，提升可读性。关闭后会尽量保留原始识别结果。")
                    }
                }
            }
        }
        .formStyle(.grouped)
        .padding(16)
    }
}

private struct SettingsRootView: View {
    @ObservedObject var store: SettingsStore

    var body: some View {
        VStack(spacing: 0) {
            TabView {
                TriggerInteractionSettingsView(config: $store.config)
                    .tabItem { Label("快捷键", systemImage: "keyboard") }

                RecognitionTextSettingsView(config: $store.config)
                    .tabItem { Label("文本质量", systemImage: "text.bubble") }
            }

            Divider()
            HStack {
                Image(systemName: {
                    switch store.statusLevel {
                    case .info: return "info.circle"
                    case .success: return "checkmark.circle.fill"
                    case .error: return "xmark.octagon.fill"
                    }
                }())
                .foregroundStyle({
                    switch store.statusLevel {
                    case .info: return Color.secondary
                    case .success: return Color.green
                    case .error: return Color.red
                    }
                }())
                .help(store.status)

                Text(store.lastSavedAt)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)

                Spacer()
                Button("重新加载") { store.load() }
                Button("保存") { store.save() }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut("s", modifiers: .command)
            }
            .padding(12)
        }
        .frame(minWidth: 720, minHeight: 560)
    }
}

private final class SettingsAppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow?

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }
}

private enum SettingsStoreRegistry {
    static let shared = SettingsStore()
}

@discardableResult
func makeSettingsWindow() -> NSWindow {
    let store = SettingsStoreRegistry.shared
    store.load()
    let rootView = SettingsRootView(store: store)
    let hosting = NSHostingView(rootView: rootView)

    let window = NSWindow(
        contentRect: NSRect(x: 0, y: 0, width: 760, height: 620),
        styleMask: [.titled, .closable, .miniaturizable, .resizable],
        backing: .buffered,
        defer: false
    )
    window.title = "Shuo 设置"
    window.center()
    window.contentView = hosting
    window.makeKeyAndOrderFront(nil)
    return window
}

func runSettingsUI() {
    let app = NSApplication.shared
    let delegate = SettingsAppDelegate()
    app.delegate = delegate
    app.setActivationPolicy(.regular)
    let window = makeSettingsWindow()
    delegate.window = window

    app.activate(ignoringOtherApps: true)
    app.run()
}
