import Foundation

func runtimeTimelineNowMs() -> UInt64 {
    UInt64((Date().timeIntervalSince1970 * 1000).rounded())
}

func runtimeTimelineUptimeMs() -> UInt64 {
    DispatchTime.now().uptimeNanoseconds / 1_000_000
}

func runtimeTimelineTextPreview(_ text: String, limit: Int = 80) -> String {
    let normalized = text
        .replacingOccurrences(of: "\n", with: "↩︎")
        .replacingOccurrences(of: "\t", with: "⇥")
        .trimmingCharacters(in: .whitespacesAndNewlines)
    guard normalized.count > limit else {
        return normalized
    }
    return String(normalized.prefix(limit)) + "…"
}

func runtimeTimelineTextFields(_ text: String) -> [String: Any] {
    [
        "chars": text.count,
        "preview": runtimeTimelineTextPreview(text),
    ]
}

struct RuntimeTimelineLatestReport: Encodable {
    let path: String
    let lineCount: Int
    let lines: [String]
}

final class RuntimeTimeline {
    static let shared = RuntimeTimeline()

    private let queue = DispatchQueue(label: "hj.voice.timeline", qos: .utility)
    private let sessionID = UUID().uuidString.lowercased()
    private let startedAtMs = runtimeTimelineNowMs()
    private let fileURL: URL
    private let formatter = ISO8601DateFormatter()
    private var handle: FileHandle?
    private var sequence: UInt64 = 0

    private init() {
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let directory = Self.directoryURL()
        let filename = "runtime-\(startedAtMs)-pid\(ProcessInfo.processInfo.processIdentifier).jsonl"
        self.fileURL = directory.appendingPathComponent(filename)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        FileManager.default.createFile(atPath: fileURL.path, contents: nil)
        self.handle = try? FileHandle(forWritingTo: fileURL)
        self.handle?.seekToEndOfFile()
        pruneOldFiles(in: directory, keeping: 20)
        record("timeline", "started", fields: [
            "file": fileURL.path,
            "pid": ProcessInfo.processInfo.processIdentifier,
        ])
    }

    func currentFilePath() -> String {
        fileURL.path
    }

    func record(_ category: String, _ name: String, fields: [String: Any] = [:]) {
        queue.async { [weak self] in
            self?.append(category: category, name: name, fields: fields)
        }
    }

    private func append(category: String, name: String, fields: [String: Any]) {
        sequence += 1
        let now = Date()
        let payload: [String: Any] = [
            "session_id": sessionID,
            "seq": sequence,
            "ts": formatter.string(from: now),
            "ts_ms": runtimeTimelineNowMs(),
            "uptime_ms": runtimeTimelineUptimeMs(),
            "category": category,
            "name": name,
            "fields": sanitize(fields),
        ]
        guard JSONSerialization.isValidJSONObject(payload),
              let data = try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys]) else {
            return
        }
        guard let handle else { return }
        try? handle.write(contentsOf: data)
        try? handle.write(contentsOf: Data([0x0a]))
    }

    private func sanitize(_ value: Any) -> Any {
        switch value {
        case is NSNull:
            return NSNull()
        case let value as String:
            return value
        case let value as NSNumber:
            if CFGetTypeID(value) == CFBooleanGetTypeID() {
                return value.boolValue
            }
            let string = value.stringValue
            if string.contains(".") || string.contains("e") || string.contains("E") {
                return value.doubleValue
            }
            return value.int64Value
        case let value as Bool:
            return value
        case let value as Int:
            return value
        case let value as Int8:
            return Int(value)
        case let value as Int16:
            return Int(value)
        case let value as Int32:
            return Int(value)
        case let value as Int64:
            return value
        case let value as UInt:
            return value
        case let value as UInt8:
            return UInt(value)
        case let value as UInt16:
            return UInt(value)
        case let value as UInt32:
            return UInt(value)
        case let value as UInt64:
            return value
        case let value as Double:
            return value
        case let value as Float:
            return Double(value)
        case let value as URL:
            return value.path
        case let value as [String: Any]:
            var mapped: [String: Any] = [:]
            for key in value.keys.sorted() {
                if let item = value[key] {
                    mapped[key] = sanitize(item)
                }
            }
            return mapped
        case let value as [Any]:
            return value.map { sanitize($0) }
        default:
            return String(describing: value)
        }
    }

    static func directoryURL() -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("Library/Application Support")
        return base
            .appendingPathComponent("hj-voice", isDirectory: true)
            .appendingPathComponent("diagnostics", isDirectory: true)
            .appendingPathComponent("timeline", isDirectory: true)
    }

    static func latestReport(limit: Int) -> RuntimeTimelineLatestReport? {
        guard let url = latestFileURL() else { return nil }
        guard let raw = try? String(contentsOf: url, encoding: .utf8) else { return nil }
        let lines = raw.split(whereSeparator: \.isNewline).map(String.init)
        let capped = max(1, limit)
        return RuntimeTimelineLatestReport(
            path: url.path,
            lineCount: min(capped, lines.count),
            lines: Array(lines.suffix(capped))
        )
    }

    private static func latestFileURL() -> URL? {
        let directory = directoryURL()
        guard let urls = try? FileManager.default.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: [.contentModificationDateKey],
            options: [.skipsHiddenFiles]
        ) else {
            return nil
        }
        return urls.max { lhs, rhs in
            let leftDate = (try? lhs.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
            let rightDate = (try? rhs.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
            return leftDate < rightDate
        }
    }

    private func pruneOldFiles(in directory: URL, keeping limit: Int) {
        guard let urls = try? FileManager.default.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: [.contentModificationDateKey],
            options: [.skipsHiddenFiles]
        ) else {
            return
        }
        let sorted = urls.sorted { lhs, rhs in
            let leftDate = (try? lhs.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
            let rightDate = (try? rhs.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
            return leftDate > rightDate
        }
        guard sorted.count > limit else { return }
        for url in sorted.dropFirst(limit) {
            try? FileManager.default.removeItem(at: url)
        }
    }
}
