import Foundation

@main
struct ShuoBenchMain {
    static func main() {
        do {
            try run()
        } catch let error as CLIError {
            FileHandle.standardError.write(Data((error.description + "\n").utf8))
            Foundation.exit(1)
        } catch {
            FileHandle.standardError.write(Data(("unexpected error: \(error)\n").utf8))
            Foundation.exit(1)
        }
    }

    static func run() throws {
        let args = Array(CommandLine.arguments.dropFirst())
        guard !args.isEmpty else {
            throw CLIError.usage(usage())
        }
        try runLatencyBench(options: LatencyBenchOptions.parse(args))
    }
}
