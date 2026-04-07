import Carbon
import Foundation

func inputSourceProperty(_ source: TISInputSource, key: CFString) -> CFTypeRef? {
    guard let raw = TISGetInputSourceProperty(source, key) else {
        return nil
    }
    return unsafeBitCast(raw, to: CFTypeRef.self)
}

func inputSourceString(_ source: TISInputSource, key: CFString) -> String? {
    inputSourceProperty(source, key: key) as? String
}

func inputSourceBool(_ source: TISInputSource, key: CFString) -> Bool? {
    inputSourceProperty(source, key: key) as? Bool
}

func allInputSources() -> [TISInputSource] {
    let array = TISCreateInputSourceList(nil, false).takeRetainedValue() as NSArray
    return array.map { $0 as! TISInputSource }
}

func inputSourceInfo(_ source: TISInputSource) -> InputSourceInfo {
    InputSourceInfo(
        localizedName: inputSourceString(source, key: kTISPropertyLocalizedName),
        sourceId: inputSourceString(source, key: kTISPropertyInputSourceID),
        bundleId: inputSourceString(source, key: kTISPropertyBundleID),
        inputModeId: inputSourceString(source, key: kTISPropertyInputModeID),
        category: inputSourceString(source, key: kTISPropertyInputSourceCategory),
        type: inputSourceString(source, key: kTISPropertyInputSourceType),
        enabled: inputSourceBool(source, key: kTISPropertyInputSourceIsEnabled),
        selectable: inputSourceBool(source, key: kTISPropertyInputSourceIsSelectCapable),
        selected: inputSourceBool(source, key: kTISPropertyInputSourceIsSelected)
    )
}

func findInputSource(matcher: String, inputModeId: String?) -> TISInputSource? {
    let sources = allInputSources()
    let sourceIdMatch = sources.first { source in
        inputSourceString(source, key: kTISPropertyInputSourceID) == matcher &&
            (inputModeId == nil || inputSourceString(source, key: kTISPropertyInputModeID) == inputModeId)
    }
    if let sourceIdMatch {
        return sourceIdMatch
    }
    return sources.first { source in
        inputSourceString(source, key: kTISPropertyBundleID) == matcher &&
            (inputModeId == nil || inputSourceString(source, key: kTISPropertyInputModeID) == inputModeId)
    }
}

func isInputSourceSelected(matcher: String, inputModeId: String?) -> Bool {
    guard let source = findInputSource(matcher: matcher, inputModeId: inputModeId) else {
        return false
    }
    return inputSourceBool(source, key: kTISPropertyInputSourceIsSelected) ?? false
}

func selectInputSource(matcher: String, inputModeId: String?) throws -> InputSourceInfo {
    guard let source = findInputSource(matcher: matcher, inputModeId: inputModeId) else {
        throw CLIError.invalid("input source not found: \(matcher)")
    }
    let enabled = inputSourceBool(source, key: kTISPropertyInputSourceIsEnabled) ?? false
    if !enabled {
        let enableStatus = TISEnableInputSource(source)
        guard enableStatus == noErr else {
            throw CLIError.invalid("TISEnableInputSource failed: \(enableStatus)")
        }
    }
    let selected = inputSourceBool(source, key: kTISPropertyInputSourceIsSelected) ?? false
    if !selected {
        let selectStatus = TISSelectInputSource(source)
        guard selectStatus == noErr else {
            throw CLIError.invalid("TISSelectInputSource failed: \(selectStatus)")
        }
    }
    for _ in 0..<30 {
        if isInputSourceSelected(matcher: matcher, inputModeId: inputModeId) {
            guard let refreshed = findInputSource(matcher: matcher, inputModeId: inputModeId) else {
                break
            }
            return inputSourceInfo(refreshed)
        }
        usleep(100_000)
    }
    guard let refreshed = findInputSource(matcher: matcher, inputModeId: inputModeId) else {
        throw CLIError.invalid("input source disappeared after select: \(matcher)")
    }
    throw CLIError.invalid("input source did not become selected in time: \(inputSourceInfo(refreshed).localizedName ?? matcher)")
}
