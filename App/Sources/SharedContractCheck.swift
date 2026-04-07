import Foundation

struct SharedContractCheckReport: Encodable {
    let contextFixturePath: String
    let ipcFixturePath: String
    let contextVersion: Int
    let shortcutKey: String
    let shortcutMode: String
    let ipcLineCount: Int
    let helloEncodingOK: Bool
    let updateContextEncodingOK: Bool
}

private func resolveSharedFixtureURL(_ name: String) -> URL? {
    let fileManager = FileManager.default
    let roots: [URL?] = [
        Bundle.main.resourceURL,
        Bundle.main.bundleURL,
        Bundle.main.executableURL?.deletingLastPathComponent(),
        URL(fileURLWithPath: fileManager.currentDirectoryPath),
    ]

    for root in roots.compactMap({ $0 }) {
        var current: URL? = root
        for _ in 0..<8 {
            guard let dir = current else { break }
            let candidates = [
                dir.appendingPathComponent("Shared/Fixtures/\(name)"),
                dir.appendingPathComponent("Fixtures/\(name)"),
            ]
            if let match = candidates.first(where: { fileManager.fileExists(atPath: $0.path) }) {
                return match
            }
            current = dir.deletingLastPathComponent()
            if current == dir { break }
        }
    }
    return nil
}

private func parseJSONObject(_ data: Data) throws -> [String: Any] {
    guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
        throw CLIError.invalid("fixture is not a JSON object")
    }
    return object
}

func runSharedContractCheck() throws {
    guard let contextURL = resolveSharedFixtureURL("context-config.v1.json"),
          let ipcURL = resolveSharedFixtureURL("ipc.v1.jsonl") else {
        throw CLIError.invalid("missing Shared/Fixtures contract files")
    }

    let contextData = try Data(contentsOf: contextURL)
    let config = try JSONDecoder().decode(ContextConfig.self, from: contextData)
    guard config.version == 1,
          config.shortcut.key == "right_command",
          config.shortcut.mode == "hold",
          config.textContext.mode == "auto",
          config.textContext.maxChars == 256 else {
        throw CLIError.invalid("context fixture shape mismatch")
    }

    let helloPayload = try parseJSONObject(Data(jsonLine(EngineHostCommand.hello()).dropLast()))
    let helloEncodingOK =
        helloPayload["type"] as? String == "hello"
        && helloPayload["protocol_version"] as? Int == 1

    let snapshot = EngineContextSnapshot(
        frontmostBundleId: "com.example.App",
        textBeforeCursor: "你好",
        textAfterCursor: "世界",
        cursorPosition: 2,
        captureSource: "focused_text_ax",
        capturedAtMs: 1712345678901
    )
    let updatePayload = try parseJSONObject(Data(jsonLine(EngineHostCommand.updateContext(snapshot)).dropLast()))
    let updateContext = updatePayload["context_snapshot"] as? [String: Any]
    let updateContextEncodingOK =
        updatePayload["type"] as? String == "update_context"
        && updateContext?["frontmost_bundle_id"] as? String == "com.example.App"
        && updateContext?["text_before_cursor"] as? String == "你好"
        && updateContext?["text_after_cursor"] as? String == "世界"
        && updateContext?["cursor_position"] as? Int == 2

    let ipcText = try String(contentsOf: ipcURL, encoding: .utf8)
    let lines = ipcText.split(whereSeparator: \.isNewline).map(String.init)
    let allowedTypes: Set<String> = [
        "hello", "ready", "update_context", "start_recording", "partial", "final", "recording_stopped",
    ]
    for line in lines {
        let object = try parseJSONObject(Data(line.utf8))
        guard let type = object["type"] as? String, allowedTypes.contains(type) else {
            throw CLIError.invalid("unexpected ipc fixture line: \(line)")
        }
    }

    print(try toJSON(SharedContractCheckReport(
        contextFixturePath: contextURL.path,
        ipcFixturePath: ipcURL.path,
        contextVersion: config.version,
        shortcutKey: config.shortcut.key,
        shortcutMode: config.shortcut.mode,
        ipcLineCount: lines.count,
        helloEncodingOK: helloEncodingOK,
        updateContextEncodingOK: updateContextEncodingOK
    )))
}
