import Foundation

private struct TimelineQueryLine: Codable {
    let sessionID: String?
    let seq: UInt64?
    let ts: String?
    let tsMs: UInt64?
    let uptimeMs: UInt64?
    let category: String?
    let name: String?
    let fields: [String: JSONValue]?

    enum CodingKeys: String, CodingKey {
        case sessionID = "session_id"
        case seq
        case ts
        case tsMs = "ts_ms"
        case uptimeMs = "uptime_ms"
        case category
        case name
        case fields
    }
}

private struct TimelineSessionSummary: Encodable {
    let sessionId: String
    let startedAtMs: UInt64
    let endedAtMs: UInt64?
    let startLine: TimelineQueryLine
    let endLine: TimelineQueryLine?
    let firstPartialAtMs: UInt64?
    let firstOverlayPartialAtMs: UInt64?
    let finalAtMs: UInt64?
    let injectionAtMs: UInt64?
    let firstPartialLatencyMs: UInt64?
    let firstOverlayPartialLatencyMs: UInt64?
    let finalLatencyMs: UInt64?
    let injectionLatencyMs: UInt64?
    let lineCount: Int
}

private enum JSONValue: Codable {
    case string(String)
    case number(Double)
    case bool(Bool)
    case object([String: JSONValue])
    case array([JSONValue])
    case null

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Double.self) {
            self = .number(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else if let value = try? container.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? container.decode([String: JSONValue].self) {
            self = .object(value)
        } else if let value = try? container.decode([JSONValue].self) {
            self = .array(value)
        } else {
            throw DecodingError.typeMismatch(
                JSONValue.self,
                DecodingError.Context(codingPath: decoder.codingPath, debugDescription: "unsupported json")
            )
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .string(let value):
            try container.encode(value)
        case .number(let value):
            try container.encode(value)
        case .bool(let value):
            try container.encode(value)
        case .object(let value):
            try container.encode(value)
        case .array(let value):
            try container.encode(value)
        case .null:
            try container.encodeNil()
        }
    }

    var stringValue: String? {
        if case .string(let value) = self { return value }
        return nil
    }
}

private struct TimelineQueryReport: Encodable {
    let path: String
    let lineCount: Int
    let lines: [TimelineQueryLine]
}

func runTimelineTail(count: Int = 80) throws {
    guard let report = RuntimeTimeline.latestReport(limit: max(1, count)) else {
        let empty = TimelineQueryReport(
            path: RuntimeTimeline.directoryURL().path,
            lineCount: 0,
            lines: []
        )
        print(try toJSON(empty))
        return
    }
    let lines = report.lines.compactMap { line in
        try? JSONDecoder().decode(TimelineQueryLine.self, from: Data(line.utf8))
    }
    print(try toJSON(TimelineQueryReport(
        path: report.path,
        lineCount: lines.count,
        lines: lines
    )))
}

func runTimelineSessions(count: Int = 20) throws {
    guard let url = runtimeTimelineLatestFileURL(),
          let raw = try? String(contentsOf: url, encoding: .utf8) else {
        let output: [String: AnyEncodable] = [
            "path": AnyEncodable(RuntimeTimeline.directoryURL().path),
            "sessions": AnyEncodable([TimelineSessionSummary]()),
        ]
        print(try toJSON(output))
        return
    }
    let lines = raw
        .split(whereSeparator: \.isNewline)
        .compactMap { try? JSONDecoder().decode(TimelineQueryLine.self, from: Data($0.utf8)) }
    let sessions = summarizeTimelineSessions(lines).suffix(max(0, count))
    let output: [String: AnyEncodable] = [
        "path": AnyEncodable(url.path),
        "sessions": AnyEncodable(Array(sessions)),
    ]
    print(try toJSON(output))
}

private func summarizeTimelineSessions(_ lines: [TimelineQueryLine]) -> [TimelineSessionSummary] {
    var summaries: [TimelineSessionSummary] = []
    var index = 0
    while index < lines.count {
        let line = lines[index]
        guard line.category == "host",
              line.name == "start_recording_requested",
              let sessionId = line.fields?["session_id"]?.stringValue,
              let startedAtMs = line.tsMs else {
            index += 1
            continue
        }

        let endIndex = findTimelineSessionEndIndex(lines, startIndex: index + 1, sessionId: sessionId)
        let slice = Array(lines[index..<(endIndex.map { $0 + 1 } ?? lines.count)])
        let endedAtMs = endIndex.flatMap { lines[$0].tsMs }
        let endLine = endIndex.map { lines[$0] }
        let firstPartialAtMs = slice.first(where: {
            $0.category == "engine_event" && $0.name == "partial"
        })?.tsMs
        let firstOverlayPartialAtMs = slice.first(where: {
            $0.category == "overlay" && $0.name == "partial_render"
        })?.tsMs
        let finalAtMs = slice.first(where: {
            $0.category == "engine_event" && $0.name == "final"
        })?.tsMs
        let injectionAtMs = slice.first(where: {
            $0.category == "ui" && $0.name == "final_injection"
        })?.tsMs
        summaries.append(TimelineSessionSummary(
            sessionId: sessionId,
            startedAtMs: startedAtMs,
            endedAtMs: endedAtMs,
            startLine: line,
            endLine: endLine,
            firstPartialAtMs: firstPartialAtMs,
            firstOverlayPartialAtMs: firstOverlayPartialAtMs,
            finalAtMs: finalAtMs,
            injectionAtMs: injectionAtMs,
            firstPartialLatencyMs: deltaMs(from: startedAtMs, to: firstPartialAtMs),
            firstOverlayPartialLatencyMs: deltaMs(from: startedAtMs, to: firstOverlayPartialAtMs),
            finalLatencyMs: deltaMs(from: startedAtMs, to: finalAtMs),
            injectionLatencyMs: deltaMs(from: startedAtMs, to: injectionAtMs),
            lineCount: slice.count
        ))
        index = (endIndex ?? index) + 1
    }
    return summaries
}

private func findTimelineSessionEndIndex(
    _ lines: [TimelineQueryLine],
    startIndex: Int,
    sessionId: String
) -> Int? {
    for index in startIndex..<lines.count {
        let line = lines[index]
        guard let fields = line.fields,
              fields["session_id"]?.stringValue == sessionId,
              line.category == "engine_event"
        else {
            continue
        }
        if line.name == "final" {
            return index
        }
        if line.name == "recording_stopped" {
            let reason = fields["reason"]?.stringValue ?? ""
            if reason != "flush_pending" {
                return index
            }
        }
    }
    return nil
}

private func deltaMs(from start: UInt64, to end: UInt64?) -> UInt64? {
    guard let end, end >= start else { return nil }
    return end - start
}

private func runtimeTimelineLatestFileURL() -> URL? {
    let directory = RuntimeTimeline.directoryURL()
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

private struct AnyEncodable: Encodable {
    private let encodeImpl: (Encoder) throws -> Void

    init<T: Encodable>(_ value: T) {
        self.encodeImpl = value.encode
    }

    func encode(to encoder: Encoder) throws {
        try encodeImpl(encoder)
    }
}
