import AppKit
import Foundation
import SwiftUI

struct TranscriptHistoryEntry: Codable, Identifiable, Hashable {
    let id: UUID
    let text: String
    let createdAt: Date
}

final class TranscriptHistoryStore: ObservableObject {
    @Published private(set) var entries: [TranscriptHistoryEntry] = []

    private let maxCount = 100
    private let storageURL: URL

    init() {
        self.storageURL = Self.makeStorageURL()
        load()
    }

    func add(_ rawText: String) {
        let text = normalize(rawText)
        guard !text.isEmpty else { return }
        entries.insert(
            TranscriptHistoryEntry(id: UUID(), text: text, createdAt: Date()),
            at: 0
        )
        if entries.count > maxCount {
            entries = Array(entries.prefix(maxCount))
        }
        save()
    }

    func filtered(query: String) -> [TranscriptHistoryEntry] {
        let needle = normalize(query)
        guard !needle.isEmpty else {
            return entries
        }
        return entries.filter { $0.text.localizedCaseInsensitiveContains(needle) }
    }

    func copy(_ text: String) {
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
    }

    func copy(entry: TranscriptHistoryEntry) {
        copy(entry.text)
    }

    private func load() {
        guard let data = try? Data(contentsOf: storageURL),
              let decoded = try? JSONDecoder().decode([TranscriptHistoryEntry].self, from: data) else {
            entries = []
            return
        }
        entries = Array(decoded.prefix(maxCount))
    }

    private func save() {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let data = try? encoder.encode(entries) else { return }
        try? FileManager.default.createDirectory(
            at: storageURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try? data.write(to: storageURL, options: [.atomic])
    }

    private func normalize(_ raw: String) -> String {
        raw
            .replacingOccurrences(of: "\n", with: " ")
            .replacingOccurrences(of: "\t", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func makeStorageURL() -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("Library/Application Support")
        return base
            .appendingPathComponent("shuo", isDirectory: true)
            .appendingPathComponent("recent_transcripts.json")
    }
}

private struct TranscriptHistorySearchView: View {
    @ObservedObject var store: TranscriptHistoryStore
    @State private var query = ""
    @State private var copiedID: UUID?

    private var results: [TranscriptHistoryEntry] {
        store.filtered(query: query)
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                TextField("搜索最近转录", text: $query)
                    .textFieldStyle(.roundedBorder)
                Text("\(results.count) 条")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            .padding(12)

            Divider()

            List(results) { entry in
                Button {
                    store.copy(entry: entry)
                    copiedID = entry.id
                } label: {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(entry.text)
                            .foregroundStyle(.primary)
                            .multilineTextAlignment(.leading)
                        HStack(spacing: 8) {
                            Text(entry.createdAt.formatted(date: .omitted, time: .shortened))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            if copiedID == entry.id {
                                Text("已复制")
                                    .font(.caption)
                                    .foregroundStyle(.blue)
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, 4)
                }
                .buttonStyle(.plain)
            }
            .overlay {
                if results.isEmpty {
                    VStack(spacing: 8) {
                        Image(systemName: "magnifyingglass")
                            .font(.system(size: 28))
                            .foregroundStyle(.secondary)
                        Text("没有匹配结果")
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
        .frame(minWidth: 560, minHeight: 420)
    }
}

@discardableResult
func makeTranscriptHistoryWindow(store: TranscriptHistoryStore) -> NSWindow {
    let rootView = TranscriptHistorySearchView(store: store)
    let hosting = NSHostingView(rootView: rootView)
    let window = NSWindow(
        contentRect: NSRect(x: 0, y: 0, width: 640, height: 520),
        styleMask: [.titled, .closable, .miniaturizable, .resizable],
        backing: .buffered,
        defer: false
    )
    window.title = "最近转录"
    window.center()
    window.contentView = hosting
    window.makeKeyAndOrderFront(nil)
    return window
}

func transcriptHistoryMenuTitle(for text: String, maxCount: Int = 44) -> String {
    let normalized = text
        .replacingOccurrences(of: "\n", with: " ")
        .replacingOccurrences(of: "\t", with: " ")
        .trimmingCharacters(in: .whitespacesAndNewlines)
    guard normalized.count > maxCount else {
        return normalized
    }
    return String(normalized.prefix(maxCount)) + "…"
}
