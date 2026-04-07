import Foundation

enum LatencyBenchProfile: String, CaseIterable, Encodable {
    case currentPcm = "current-pcm"
    case currentOpus = "current-opus"
    case androidPcm = "android-pcm"
    case androidOpus = "android-opus"
}

struct LatencyBenchOptions {
    var runs = 3
    var phrase = "现在我们直接测首字延迟，看看哪个参数最快。"
    var voice: String?
    var profiles = LatencyBenchProfile.allCases
    var chunkMsList = [10, 20, 40]
    var timeoutSeconds: TimeInterval = 10
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
            switch args[index] {
            case "--runs":
                index += 1
                guard index < args.count, let value = Int(args[index]), value > 0 else {
                    throw CLIError.invalid("invalid --runs")
                }
                options.runs = value
            case "--phrase":
                index += 1
                guard index < args.count, !args[index].isEmpty else {
                    throw CLIError.invalid("missing --phrase value")
                }
                options.phrase = args[index]
            case "--voice":
                index += 1
                guard index < args.count, !args[index].isEmpty else {
                    throw CLIError.invalid("missing --voice value")
                }
                options.voice = args[index]
            case "--profiles":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --profiles value")
                }
                options.profiles = try parseProfiles(args[index])
            case "--chunk-ms":
                index += 1
                guard index < args.count else {
                    throw CLIError.invalid("missing --chunk-ms value")
                }
                options.chunkMsList = try parseChunkMsList(args[index])
            case "--timeout":
                index += 1
                guard index < args.count, let value = Double(args[index]), value > 0 else {
                    throw CLIError.invalid("invalid --timeout")
                }
                options.timeoutSeconds = value
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

private struct LatencyBenchRunReport: Decodable, Encodable {
    let ok: Bool
    let profile: String
    let audioFile: String?
    let audioMs: UInt64
    let sampleCount: Int
    let authSource: String?
    let wsURL: String?
    let connectStartedAtMs: UInt64?
    let readyAtMs: UInt64?
    let firstAudioSentAtMs: UInt64?
    let finishSentAtMs: UInt64?
    let firstResultAtMs: UInt64?
    let firstPartialAtMs: UInt64?
    let firstFinalAtMs: UInt64?
    let finalAtMs: UInt64?
    let warmupMs: UInt64?
    let firstResultFrameMs: UInt64?
    let firstPartialFrameMs: UInt64?
    let firstFinalFrameMs: UInt64?
    let firstResultAfterAudioMs: UInt64?
    let inferMs: UInt64?
    let partialCount: UInt64
    let finalCount: UInt64
    let finalText: String
    let error: String?

    enum CodingKeys: String, CodingKey {
        case ok
        case profile
        case audioFile = "audio_file"
        case audioMs = "audio_ms"
        case sampleCount = "sample_count"
        case authSource = "auth_source"
        case wsURL = "ws_url"
        case connectStartedAtMs = "connect_started_at_ms"
        case readyAtMs = "ready_at_ms"
        case firstAudioSentAtMs = "first_audio_sent_at_ms"
        case finishSentAtMs = "finish_sent_at_ms"
        case firstResultAtMs = "first_result_at_ms"
        case firstPartialAtMs = "first_partial_at_ms"
        case firstFinalAtMs = "first_final_at_ms"
        case finalAtMs = "final_at_ms"
        case warmupMs = "warmup_ms"
        case firstResultFrameMs = "first_result_frame_ms"
        case firstPartialFrameMs = "first_partial_frame_ms"
        case firstFinalFrameMs = "first_final_frame_ms"
        case firstResultAfterAudioMs = "first_result_after_audio_ms"
        case inferMs = "infer_ms"
        case partialCount = "partial_count"
        case finalCount = "final_count"
        case finalText = "final_text"
        case error
    }
}

private struct LatencyBenchRunRecord: Encodable {
    let profile: String
    let chunkMs: Int
    let runIndex: Int
    let report: LatencyBenchRunReport

    enum CodingKeys: String, CodingKey {
        case profile
        case chunkMs = "chunk_ms"
        case runIndex = "run_index"
        case report
    }
}

private struct LatencyBenchVariantSummary: Encodable {
    let profile: String
    let chunkMs: Int
    let okRuns: Int
    let totalRuns: Int
    let firstResultAfterAudioP50Ms: Double?
    let firstResultAfterAudioP95Ms: Double?
    let firstResultFrameP50Ms: Double?
    let inferP50Ms: Double?
    let inferP95Ms: Double?

    enum CodingKeys: String, CodingKey {
        case profile
        case chunkMs = "chunk_ms"
        case okRuns = "ok_runs"
        case totalRuns = "total_runs"
        case firstResultAfterAudioP50Ms = "first_result_after_audio_p50_ms"
        case firstResultAfterAudioP95Ms = "first_result_after_audio_p95_ms"
        case firstResultFrameP50Ms = "first_result_frame_p50_ms"
        case inferP50Ms = "infer_p50_ms"
        case inferP95Ms = "infer_p95_ms"
    }
}

private struct LatencyBenchSummary: Encodable {
    let generatedAtMs: UInt64
    let phrase: String
    let voice: String
    let runsPerVariant: Int
    let chunkMsList: [Int]
    let profiles: [String]
    let bestProfile: String?
    let bestChunkMs: Int?
    let variants: [LatencyBenchVariantSummary]
    let rawRuns: [LatencyBenchRunRecord]

    enum CodingKeys: String, CodingKey {
        case generatedAtMs = "generated_at_ms"
        case phrase
        case voice
        case runsPerVariant = "runs_per_variant"
        case chunkMsList = "chunk_ms_list"
        case profiles
        case bestProfile = "best_profile"
        case bestChunkMs = "best_chunk_ms"
        case variants
        case rawRuns = "raw_runs"
    }
}

func runLatencyBench(options: LatencyBenchOptions) throws {
    let fileManager = FileManager.default
    let rootURL = URL(fileURLWithPath: fileManager.currentDirectoryPath)
    let docsDirURL = rootURL.appendingPathComponent("docs/latency-bench", isDirectory: true)
    try fileManager.createDirectory(at: docsDirURL, withIntermediateDirectories: true, attributes: nil)

    let tempDirURL = rootURL
        .appendingPathComponent(".build", isDirectory: true)
        .appendingPathComponent("latency-bench-\(currentTimeMillis())", isDirectory: true)
    try fileManager.createDirectory(at: tempDirURL, withIntermediateDirectories: true, attributes: nil)

    let environment = latencyBenchEnvironment(rootURL: rootURL)

    let voice = try resolveLatencyBenchVoice(preferred: options.voice)
    let wavURL = try synthesizeLatencyBenchAudio(
        phrase: options.phrase,
        voice: voice,
        workingDirectoryURL: tempDirURL
    )
    let helperURL = try prepareLatencyBenchHelper(
        rootURL: rootURL,
        helperBin: options.helperBin,
        environment: environment
    )

    var rawRuns: [LatencyBenchRunRecord] = []
    for profile in options.profiles {
        for chunkMs in options.chunkMsList {
            for runIndex in 0..<options.runs {
                let report = try executeLatencyBenchRun(
                    helperURL: helperURL,
                    rootURL: rootURL,
                    wavURL: wavURL,
                    profile: profile,
                    chunkMs: chunkMs,
                    options: options,
                    environment: environment
                )
                rawRuns.append(.init(
                    profile: profile.rawValue,
                    chunkMs: chunkMs,
                    runIndex: runIndex + 1,
                    report: report
                ))
            }
        }
    }

    let variants = summarizeLatencyBenchVariants(rawRuns)
    let bestVariant = variants.min {
        compareLatencyBenchVariants($0, $1)
    }
    let summary = LatencyBenchSummary(
        generatedAtMs: currentTimeMillis(),
        phrase: options.phrase,
        voice: voice,
        runsPerVariant: options.runs,
        chunkMsList: options.chunkMsList,
        profiles: options.profiles.map(\.rawValue),
        bestProfile: bestVariant?.profile,
        bestChunkMs: bestVariant?.chunkMs,
        variants: variants,
        rawRuns: rawRuns
    )

    let summaryData = try JSONEncoder.prettySorted.encode(summary)
    let latestURL = docsDirURL.appendingPathComponent("latest.json")
    try summaryData.write(to: latestURL, options: .atomic)
    print(String(decoding: summaryData, as: UTF8.self))
}

private func parseProfiles(_ raw: String) throws -> [LatencyBenchProfile] {
    let items = raw
        .split(separator: ",")
        .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
        .filter { !$0.isEmpty }
    guard !items.isEmpty else {
        throw CLIError.invalid("invalid --profiles")
    }
    let profiles = try items.map { item -> LatencyBenchProfile in
        guard let profile = LatencyBenchProfile(rawValue: item) else {
            throw CLIError.invalid("unknown profile: \(item)")
        }
        return profile
    }
    return profiles
}

private func parseChunkMsList(_ raw: String) throws -> [Int] {
    let values = raw
        .split(separator: ",")
        .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
        .filter { !$0.isEmpty }
    let chunkMs = try values.map { item -> Int in
        guard let value = Int(item), value >= 10 else {
            throw CLIError.invalid("invalid chunk_ms: \(item)")
        }
        return value
    }
    guard !chunkMs.isEmpty else {
        throw CLIError.invalid("invalid --chunk-ms")
    }
    return Array(Set(chunkMs)).sorted()
}

private func resolveLatencyBenchVoice(preferred: String?) throws -> String {
    if let preferred, !preferred.isEmpty {
        return preferred
    }
    let output = try runProcessCapture(
        executable: "/usr/bin/say",
        arguments: ["-v", "?"]
    )
    let lines = output.stdout.split(whereSeparator: \.isNewline).map(String.init)
    if lines.contains(where: { $0.hasPrefix("Tingting") }) {
        return "Tingting"
    }
    if let fallback = lines.first(where: { $0.contains(" zh_CN ") })?.split(separator: " ").first {
        return String(fallback)
    }
    throw CLIError.invalid("no usable Chinese voice from say -v ?")
}

private func synthesizeLatencyBenchAudio(
    phrase: String,
    voice: String,
    workingDirectoryURL: URL
) throws -> URL {
    let aiffURL = workingDirectoryURL.appendingPathComponent("latency-bench.aiff")
    let wavURL = workingDirectoryURL.appendingPathComponent("latency-bench.wav")
    _ = try runProcessCapture(
        executable: "/usr/bin/say",
        arguments: ["-v", voice, "-o", aiffURL.path, phrase]
    )
    _ = try runProcessCapture(
        executable: "/usr/bin/afconvert",
        arguments: ["-f", "WAVE", "-d", "LEI16@16000", "-c", "1", aiffURL.path, wavURL.path]
    )
    return wavURL
}

private func prepareLatencyBenchHelper(
    rootURL: URL,
    helperBin: String?,
    environment: [String: String]
) throws -> URL {
    if let helperBin, !helperBin.isEmpty {
        return URL(fileURLWithPath: helperBin)
    }
    let manifestPath = rootURL.appendingPathComponent("Engine/shuo-engine/Cargo.toml").path
    _ = try runProcessCapture(
        executable: "/usr/bin/env",
        arguments: ["cargo", "build", "--quiet", "--features", "latency-bench", "--manifest-path", manifestPath],
        currentDirectoryURL: rootURL,
        environment: environment
    )
    let helperURL = rootURL.appendingPathComponent("Engine/shuo-engine/target/debug/shuo-engine")
    guard FileManager.default.isExecutableFile(atPath: helperURL.path) else {
        throw CLIError.invalid("benchmark helper missing: \(helperURL.path)")
    }
    return helperURL
}

private func executeLatencyBenchRun(
    helperURL: URL,
    rootURL: URL,
    wavURL: URL,
    profile: LatencyBenchProfile,
    chunkMs: Int,
    options: LatencyBenchOptions,
    environment: [String: String]
) throws -> LatencyBenchRunReport {
    var arguments = [
        "--mode", "benchmark-replay",
        "--transport", "direct-frontier",
        "--frontier-profile", profile.rawValue,
        "--benchmark-input-wav", wavURL.path,
        "--benchmark-chunk-ms", String(chunkMs),
        "--benchmark-timeout-secs", String(options.timeoutSeconds),
    ]
    if !options.warmup {
        arguments.append("--benchmark-warmup=false")
    }
    if let frontierToken = options.frontierToken {
        arguments += ["--frontier-token", frontierToken]
    }
    if let frontierAppKey = options.frontierAppKey {
        arguments += ["--frontier-app-key", frontierAppKey]
    }
    if let bootstrapEnv = options.bootstrapEnv {
        arguments += ["--bootstrap-env", bootstrapEnv]
    }
    if let authCache = options.authCache {
        arguments += ["--auth-cache-path", authCache]
    }
    if let desktopSessionEnv = options.desktopSessionEnv {
        arguments += ["--desktop-session-env", desktopSessionEnv]
    }
    if options.enableMacLiveAuth {
        arguments.append("--enable-mac-live-auth")
    }
    if let macLiveTokenScript = options.macLiveTokenScript {
        arguments += ["--mac-live-token-script", macLiveTokenScript]
    }
    if options.disableAndroidVdeviceAuth {
        arguments.append("--disable-android-vdevice-auth")
    }
    let output = try runProcessCapture(
        executable: helperURL.path,
        arguments: arguments,
        currentDirectoryURL: rootURL,
        environment: environment
    )
    let data = Data(output.stdout.utf8)
    do {
        return try JSONDecoder().decode(LatencyBenchRunReport.self, from: data)
    } catch {
        let preview = output.stdout.isEmpty ? output.stderr : output.stdout
        throw CLIError.invalid("decode benchmark report failed: \(error)\n\(preview)")
    }
}

private func summarizeLatencyBenchVariants(
    _ rawRuns: [LatencyBenchRunRecord]
) -> [LatencyBenchVariantSummary] {
    let grouped = Dictionary(grouping: rawRuns) { "\($0.profile)#\($0.chunkMs)" }
    return grouped.values
        .compactMap { records in
            guard let first = records.first else { return nil }
            let successReports = records
                .map(\.report)
                .filter(\.ok)
            let firstResultAfterAudio = successReports.compactMap { $0.firstResultAfterAudioMs.map { Double($0) } }
            let firstResultFrame = successReports.compactMap { $0.firstResultFrameMs.map { Double($0) } }
            let infer = successReports.compactMap { $0.inferMs.map { Double($0) } }
            return LatencyBenchVariantSummary(
                profile: first.profile,
                chunkMs: first.chunkMs,
                okRuns: successReports.count,
                totalRuns: records.count,
                firstResultAfterAudioP50Ms: percentile(firstResultAfterAudio, 0.50),
                firstResultAfterAudioP95Ms: percentile(firstResultAfterAudio, 0.95),
                firstResultFrameP50Ms: percentile(firstResultFrame, 0.50),
                inferP50Ms: percentile(infer, 0.50),
                inferP95Ms: percentile(infer, 0.95)
            )
        }
        .sorted {
            compareLatencyBenchVariants($0, $1)
        }
}

private func compareLatencyBenchVariants(
    _ lhs: LatencyBenchVariantSummary,
    _ rhs: LatencyBenchVariantSummary
) -> Bool {
    let lhsScore = lhs.firstResultAfterAudioP50Ms ?? .greatestFiniteMagnitude
    let rhsScore = rhs.firstResultAfterAudioP50Ms ?? .greatestFiniteMagnitude
    if lhsScore != rhsScore {
        return lhsScore < rhsScore
    }
    let lhsP95 = lhs.firstResultAfterAudioP95Ms ?? .greatestFiniteMagnitude
    let rhsP95 = rhs.firstResultAfterAudioP95Ms ?? .greatestFiniteMagnitude
    if lhsP95 != rhsP95 {
        return lhsP95 < rhsP95
    }
    let lhsInfer = lhs.inferP50Ms ?? .greatestFiniteMagnitude
    let rhsInfer = rhs.inferP50Ms ?? .greatestFiniteMagnitude
    if lhsInfer != rhsInfer {
        return lhsInfer < rhsInfer
    }
    return lhs.chunkMs < rhs.chunkMs
}

private func percentile(_ values: [Double], _ p: Double) -> Double? {
    guard !values.isEmpty else {
        return nil
    }
    let sorted = values.sorted()
    if sorted.count == 1 {
        return sorted[0]
    }
    let clamped = max(0, min(1, p))
    let position = Double(sorted.count - 1) * clamped
    let lowerIndex = Int(position.rounded(.down))
    let upperIndex = Int(position.rounded(.up))
    if lowerIndex == upperIndex {
        return sorted[lowerIndex]
    }
    let fraction = position - Double(lowerIndex)
    return sorted[lowerIndex] + (sorted[upperIndex] - sorted[lowerIndex]) * fraction
}

private func latencyBenchEnvironment(rootURL: URL) -> [String: String] {
    var environment = ProcessInfo.processInfo.environment
    let pkgConfigDirs = [
        "/opt/homebrew/lib/pkgconfig",
        "/usr/local/lib/pkgconfig",
    ].filter { FileManager.default.fileExists(atPath: $0) }
    if !pkgConfigDirs.isEmpty {
        let existing = environment["PKG_CONFIG_PATH"].map { [$0] } ?? []
        environment["PKG_CONFIG_PATH"] = (pkgConfigDirs + existing)
            .filter { !$0.isEmpty }
            .joined(separator: ":")
    }
    let pkgConfigShim = rootURL.appendingPathComponent("Engine/shuo-engine/scripts/pkg-config-opus.sh")
    if FileManager.default.isExecutableFile(atPath: pkgConfigShim.path) {
        environment["PKG_CONFIG"] = pkgConfigShim.path
    }
    let fallbackLibraryDirs = [
        "/opt/homebrew/lib",
        "/usr/local/lib",
    ].filter { FileManager.default.fileExists(atPath: $0) }
    if !fallbackLibraryDirs.isEmpty {
        environment["DYLD_FALLBACK_LIBRARY_PATH"] = fallbackLibraryDirs.joined(separator: ":")
    }
    return environment
}

private func runProcessCapture(
    executable: String,
    arguments: [String],
    currentDirectoryURL: URL? = nil,
    environment: [String: String]? = nil
) throws -> (stdout: String, stderr: String) {
    let process = Process()
    let stdoutPipe = Pipe()
    let stderrPipe = Pipe()
    process.executableURL = URL(fileURLWithPath: executable)
    process.arguments = arguments
    process.standardOutput = stdoutPipe
    process.standardError = stderrPipe
    if let currentDirectoryURL {
        process.currentDirectoryURL = currentDirectoryURL
    }
    if let environment {
        process.environment = environment
    }
    try process.run()
    process.waitUntilExit()
    let stdout = String(decoding: stdoutPipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
    let stderr = String(decoding: stderrPipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
    guard process.terminationStatus == 0 else {
        throw CLIError.invalid(
            ([stderr.trimmingCharacters(in: .whitespacesAndNewlines), stdout.trimmingCharacters(in: .whitespacesAndNewlines)]
                .filter { !$0.isEmpty }
                .joined(separator: "\n"))
        )
    }
    return (stdout, stderr)
}

private extension JSONEncoder {
    static var prettySorted: JSONEncoder {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        return encoder
    }
}
