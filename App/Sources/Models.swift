import Foundation

enum CLIError: Error, CustomStringConvertible {
    case usage(String)
    case timeout(String)
    case invalid(String)

    var description: String {
        switch self {
        case .usage(let text), .timeout(let text), .invalid(let text):
            return text
        }
    }
}

enum NotificationName {
    static let requestSnapshot = "DoubaoImeSettings.requestUserSettingsSnapshot"
    static let respondSnapshot = "DoubaoImeSettings.respondUserSettingsSnapshot"
    static let selectedMicrophoneId = "DoubaoImeSettings.selectedMicrophoneId"
    static let startMicrophoneMonitor = "DoubaoImeSettings.startMicrophoneMonitor"
    static let stopMicrophoneMonitor = "DoubaoImeSettings.stopMicrophoneMonitor"
    static let enableStartASRShortcut = "DoubaoImeSettings.enableStartASRShortcutNotification"
    static let enableGlobalASRShortcut = "DoubaoImeSettings.enableGloableASRShortcutNotification"
    static let asrShortcutKey = "DoubaoImeSettings.asrShortcutKeyNotification"
}

struct SettingsSnapshot: Encodable, Hashable {
    let requestId: String
    let settings: [String: String]
}

struct InputSourceInfo: Encodable {
    let localizedName: String?
    let sourceId: String?
    let bundleId: String?
    let inputModeId: String?
    let category: String?
    let type: String?
    let enabled: Bool?
    let selectable: Bool?
    let selected: Bool?
}

struct AXRangeInfo: Encodable {
    let location: Int
    let length: Int
}

struct FocusedTextContext: Encodable {
    let trusted: Bool
    let frontmostAppBundleId: String?
    let frontmostAppLocalizedName: String?
    let focusedRole: String?
    let focusedSubrole: String?
    let selectedRange: AXRangeInfo?
    let textBeforeCursor: String?
    let textAfterCursor: String?
    let selectedText: String?
    let textWindow: String?
    let cursorPosition: Int?
    let captureSource: String
}
