import Carbon
import Foundation

enum KeyPreset: String {
    case optionLeft = "option-left"
    case optionRight = "option-right"
    case rightCommand = "right-command"

    init?(cliValue: String) {
        switch cliValue {
        case "option-left", "optionLeft":
            self = .optionLeft
        case "option-right", "optionRight":
            self = .optionRight
        case "right-command", "rightCommand":
            self = .rightCommand
        default:
            return nil
        }
    }

    var keyCode: CGKeyCode {
        switch self {
        case .optionLeft: return 58
        case .optionRight: return 61
        case .rightCommand: return 54
        }
    }

    var flags: CGEventFlags {
        switch self {
        case .optionLeft, .optionRight:
            return .maskAlternate
        case .rightCommand:
            return .maskCommand
        }
    }

    var display: String {
        switch self {
        case .optionLeft, .optionRight:
            return "Option"
        case .rightCommand:
            return "RightCommand"
        }
    }
}

func postKeyEvent(code: CGKeyCode, flags: CGEventFlags, down: Bool) throws {
    guard let event = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down) else {
        throw CLIError.invalid("failed to create CGEvent for keyCode=\(code)")
    }
    event.flags = down ? flags : []
    event.post(tap: .cghidEventTap)
}

func pressRawKey(code: CGKeyCode, flags: CGEventFlags, holdMs: useconds_t) throws {
    try postKeyEvent(code: code, flags: flags, down: true)
    usleep(holdMs * 1_000)
    try postKeyEvent(code: code, flags: flags, down: false)
}

func pressModifier(_ preset: KeyPreset, mode: String) throws {
    switch mode {
    case "long":
        try pressRawKey(code: preset.keyCode, flags: preset.flags, holdMs: 700)
    case "double":
        for index in 0..<2 {
            try pressRawKey(code: preset.keyCode, flags: preset.flags, holdMs: 50)
            if index == 0 {
                usleep(120_000)
            }
        }
    default:
        throw CLIError.usage("unknown press mode: \(mode)")
    }
}
