import Foundation

let feedbackSoundOffValue = "off"
let defaultFeedbackStartSoundName = "jbl_begin_short.caf"
let defaultFeedbackStopSoundName = "Pop.aiff"

struct FeedbackSoundOption: Identifiable, Hashable {
    let label: String
    let value: String
    var id: String { value }
}

private let feedbackSoundExtensions = Set(["caf", "aiff", "wav", "m4a", "mp3"])
private let preferredFeedbackSoundOrder = [
    "jbl_begin_short.caf",
    "Pop.aiff",
    "jbl_confirm.caf",
    "jbl_begin.caf",
    "jbl_cancel.caf",
]

private func feedbackSoundSearchDirectories() -> [URL] {
    let fileManager = FileManager.default
    var directories: [URL] = []
    if let resourceURL = Bundle.main.resourceURL {
        directories.append(resourceURL.appendingPathComponent("sounds", isDirectory: true))
    }
    directories.append(
        URL(fileURLWithPath: fileManager.currentDirectoryPath)
            .appendingPathComponent("App/Resources/sounds", isDirectory: true)
    )
    directories.append(
        URL(fileURLWithPath: "/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/siri", isDirectory: true)
    )
    directories.append(
        URL(fileURLWithPath: "/System/Library/Sounds", isDirectory: true)
    )
    return directories
}

func resolveFeedbackSoundURL(_ fileName: String) -> URL? {
    guard !fileName.isEmpty, fileName != feedbackSoundOffValue else {
        return nil
    }
    let fileManager = FileManager.default
    for directory in feedbackSoundSearchDirectories() {
        let candidate = directory.appendingPathComponent(fileName)
        if fileManager.fileExists(atPath: candidate.path) {
            return candidate
        }
    }
    return nil
}

private func feedbackSoundLabel(for fileName: String) -> String {
    switch fileName {
    case "jbl_begin_short.caf":
        return "Siri 开始（短）"
    case "jbl_begin.caf":
        return "Siri 开始（长）"
    case "jbl_confirm.caf":
        return "Siri 结束 / 确认"
    case "jbl_cancel.caf":
        return "Siri 取消"
    default:
        return URL(fileURLWithPath: fileName).deletingPathExtension().lastPathComponent
    }
}

func availableFeedbackSoundOptions() -> [FeedbackSoundOption] {
    let fileManager = FileManager.default
    var seen = Set<String>()
    var orderedNames: [String] = []

    func append(_ fileName: String) {
        guard !fileName.isEmpty, !seen.contains(fileName), resolveFeedbackSoundURL(fileName) != nil else {
            return
        }
        seen.insert(fileName)
        orderedNames.append(fileName)
    }

    for fileName in preferredFeedbackSoundOrder {
        append(fileName)
    }

    for directory in feedbackSoundSearchDirectories() {
        guard let entries = try? fileManager.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: nil,
            options: [.skipsHiddenFiles]
        ) else {
            continue
        }
        for entry in entries.sorted(by: { $0.lastPathComponent.localizedCaseInsensitiveCompare($1.lastPathComponent) == .orderedAscending }) {
            let ext = entry.pathExtension.lowercased()
            guard feedbackSoundExtensions.contains(ext) else { continue }
            append(entry.lastPathComponent)
        }
    }

    return [FeedbackSoundOption(label: "关闭", value: feedbackSoundOffValue)] + orderedNames.map {
        FeedbackSoundOption(label: feedbackSoundLabel(for: $0), value: $0)
    }
}
