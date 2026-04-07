import AppKit
import ApplicationServices
import Foundation

struct FocusedInjectionTarget {
    let processIdentifier: pid_t
}

private struct PasteboardEntry {
    let type: NSPasteboard.PasteboardType
    let data: Data?
    let string: String?
}

func axCopyAttribute(_ element: AXUIElement, attribute: CFString) -> CFTypeRef? {
    var value: CFTypeRef?
    let error = AXUIElementCopyAttributeValue(element, attribute, &value)
    guard error == .success else {
        return nil
    }
    return value
}

func axStringAttribute(_ element: AXUIElement, attribute: CFString) -> String? {
    axCopyAttribute(element, attribute: attribute) as? String
}

func axRangeAttribute(_ element: AXUIElement, attribute: CFString) -> CFRange? {
    guard let raw = axCopyAttribute(element, attribute: attribute) else {
        return nil
    }
    let axValue = unsafeBitCast(raw, to: AXValue.self)
    guard AXValueGetType(axValue) == .cfRange else {
        return nil
    }
    var range = CFRange()
    guard AXValueGetValue(axValue, .cfRange, &range) else {
        return nil
    }
    return range
}

@discardableResult
func axSetRangeAttribute(_ element: AXUIElement, attribute: CFString, value: CFRange) -> Bool {
    var mutable = value
    guard let axValue = AXValueCreate(.cfRange, &mutable) else {
        return false
    }
    return AXUIElementSetAttributeValue(element, attribute, axValue) == .success
}

func accessibilityTrusted(prompt: Bool = false) -> Bool {
    let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: prompt] as CFDictionary
    return AXIsProcessTrustedWithOptions(options)
}

@discardableResult
func promptForAccessibilityPermission() -> Bool {
    accessibilityTrusted(prompt: true)
}

@discardableResult
func openAccessibilitySettings() -> Bool {
    let workspace = NSWorkspace.shared
    let urls = [
        URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"),
        URL(string: "x-apple.systempreferences:com.apple.Settings.PrivacySecurity.extension?Privacy_Accessibility"),
    ].compactMap { $0 }
    for url in urls where workspace.open(url) {
        return true
    }
    return false
}

func captureFocusedInjectionTarget() -> FocusedInjectionTarget? {
    guard accessibilityTrusted(),
          let frontmost = NSWorkspace.shared.frontmostApplication else {
        return nil
    }
    return FocusedInjectionTarget(processIdentifier: frontmost.processIdentifier)
}

func focusedTextContext(maxChars: Int) -> FocusedTextContext {
    let trusted = accessibilityTrusted()

    let frontmost = NSWorkspace.shared.frontmostApplication
    let bundleId = frontmost?.bundleIdentifier
    let localizedName = frontmost?.localizedName
    guard trusted, let pid = frontmost?.processIdentifier else {
        return FocusedTextContext(
            trusted: trusted,
            frontmostAppBundleId: bundleId,
            frontmostAppLocalizedName: localizedName,
            focusedRole: nil,
            focusedSubrole: nil,
            selectedRange: nil,
            textBeforeCursor: nil,
            textAfterCursor: nil,
            selectedText: nil,
            textWindow: nil,
            cursorPosition: nil,
            captureSource: trusted ? "frontmost_app_only" : "ax_untrusted"
        )
    }

    let appElement = AXUIElementCreateApplication(pid)
    guard let focusedRaw = axCopyAttribute(appElement, attribute: kAXFocusedUIElementAttribute as CFString) else {
        return FocusedTextContext(
            trusted: trusted,
            frontmostAppBundleId: bundleId,
            frontmostAppLocalizedName: localizedName,
            focusedRole: nil,
            focusedSubrole: nil,
            selectedRange: nil,
            textBeforeCursor: nil,
            textAfterCursor: nil,
            selectedText: nil,
            textWindow: nil,
            cursorPosition: nil,
            captureSource: "focused_element_missing"
        )
    }
    let focusedElement = unsafeBitCast(focusedRaw, to: AXUIElement.self)
    let role = axStringAttribute(focusedElement, attribute: kAXRoleAttribute as CFString)
    let subrole = axStringAttribute(focusedElement, attribute: kAXSubroleAttribute as CFString)
    let value = axCopyAttribute(focusedElement, attribute: kAXValueAttribute as CFString) as? String
    let selectedText = axCopyAttribute(focusedElement, attribute: kAXSelectedTextAttribute as CFString) as? String
    let selectedRange = axRangeAttribute(focusedElement, attribute: kAXSelectedTextRangeAttribute as CFString)

    var textBeforeCursor: String?
    var textAfterCursor: String?
    var textWindow: String?
    var cursorPosition: Int?
    var normalizedRange: AXRangeInfo?

    if let value, let selectedRange {
        let nsValue = value as NSString
        let lower = max(0, min(selectedRange.location, nsValue.length))
        let upper = max(lower, min(selectedRange.location + selectedRange.length, nsValue.length))
        let beforeStart = max(0, lower - maxChars)
        let beforeText = nsValue.substring(with: NSRange(location: beforeStart, length: lower - beforeStart))
        let afterCount = min(maxChars / 4, nsValue.length - upper)
        let afterText = nsValue.substring(with: NSRange(location: upper, length: afterCount))
        textBeforeCursor = beforeText
        textAfterCursor = afterText
        textWindow = beforeText + afterText
        cursorPosition = lower - beforeStart
        normalizedRange = AXRangeInfo(location: lower, length: upper - lower)
    }

    return FocusedTextContext(
        trusted: trusted,
        frontmostAppBundleId: bundleId,
        frontmostAppLocalizedName: localizedName,
        focusedRole: role,
        focusedSubrole: subrole,
        selectedRange: normalizedRange,
        textBeforeCursor: textBeforeCursor,
        textAfterCursor: textAfterCursor,
        selectedText: selectedText,
        textWindow: textWindow,
        cursorPosition: cursorPosition,
        captureSource: textBeforeCursor == nil ? "frontmost_app_only" : "focused_text_ax"
    )
}

@discardableResult
func insertTextIntoFocusedElement(_ text: String, target: FocusedInjectionTarget? = nil) -> Bool {
    guard accessibilityTrusted() else {
        return false
    }
    let pid: pid_t
    if let target {
        pid = target.processIdentifier
    } else if let frontmost = NSWorkspace.shared.frontmostApplication {
        pid = frontmost.processIdentifier
    } else {
        return false
    }

    if pasteOrTypeText(text, expectedPID: target?.processIdentifier) {
        return true
    }

    let appElement = AXUIElementCreateApplication(pid)
    guard let focusedRaw = axCopyAttribute(appElement, attribute: kAXFocusedUIElementAttribute as CFString) else {
        return false
    }
    let focusedElement = unsafeBitCast(focusedRaw, to: AXUIElement.self)

    if AXUIElementSetAttributeValue(
        focusedElement,
        kAXSelectedTextAttribute as CFString,
        text as CFTypeRef
    ) == .success {
        return true
    }

    guard let currentValue = axCopyAttribute(focusedElement, attribute: kAXValueAttribute as CFString) as? String,
          let selectedRange = axRangeAttribute(focusedElement, attribute: kAXSelectedTextRangeAttribute as CFString) else {
        return false
    }

    let nsValue = currentValue as NSString
    let lower = max(0, min(selectedRange.location, nsValue.length))
    let upper = max(lower, min(selectedRange.location + selectedRange.length, nsValue.length))
    let prefix = nsValue.substring(with: NSRange(location: 0, length: lower))
    let suffix = nsValue.substring(with: NSRange(location: upper, length: nsValue.length - upper))
    let merged = prefix + text + suffix

    guard AXUIElementSetAttributeValue(
        focusedElement,
        kAXValueAttribute as CFString,
        merged as CFTypeRef
    ) == .success else {
        return false
    }

    let insertionPoint = lower + (text as NSString).length
    if axSetRangeAttribute(
        focusedElement,
        attribute: kAXSelectedTextRangeAttribute as CFString,
        value: CFRange(location: insertionPoint, length: 0)
    ) {
        return true
    }
    return false
}

private func pasteOrTypeText(_ text: String, expectedPID: pid_t?) -> Bool {
    if let expectedPID,
       let current = NSWorkspace.shared.frontmostApplication,
       current.processIdentifier != expectedPID {
        return false
    }
    if pasteTextViaPasteboard(text) {
        return true
    }
    return typeTextViaEvents(text)
}

private func capturePasteboardSnapshot(_ pasteboard: NSPasteboard) -> [[PasteboardEntry]] {
    guard let items = pasteboard.pasteboardItems else {
        return []
    }
    return items.compactMap { item in
        let entries = item.types.compactMap { type -> PasteboardEntry? in
            if let data = item.data(forType: type) {
                return PasteboardEntry(type: type, data: data, string: nil)
            }
            if let string = item.string(forType: type) {
                return PasteboardEntry(type: type, data: nil, string: string)
            }
            return nil
        }
        return entries.isEmpty ? nil : entries
    }
}

private func restorePasteboardSnapshot(_ pasteboard: NSPasteboard, snapshot: [[PasteboardEntry]]) {
    pasteboard.clearContents()
    guard !snapshot.isEmpty else {
        return
    }
    let items: [NSPasteboardItem] = snapshot.compactMap { entries in
        let item = NSPasteboardItem()
        var wrote = false
        for entry in entries {
            if let data = entry.data {
                wrote = item.setData(data, forType: entry.type) || wrote
            } else if let string = entry.string {
                wrote = item.setString(string, forType: entry.type) || wrote
            }
        }
        return wrote ? item : nil
    }
    if !items.isEmpty {
        pasteboard.writeObjects(items)
    }
}

private func synthesizeKeyPress(keyCode: CGKeyCode, flags: CGEventFlags = []) -> Bool {
    guard let down = CGEvent(keyboardEventSource: nil, virtualKey: keyCode, keyDown: true),
          let up = CGEvent(keyboardEventSource: nil, virtualKey: keyCode, keyDown: false) else {
        return false
    }
    down.flags = flags
    up.flags = flags
    down.post(tap: .cghidEventTap)
    usleep(8_000)
    up.post(tap: .cghidEventTap)
    usleep(30_000)
    return true
}

private func pasteTextViaPasteboard(_ text: String) -> Bool {
    let pasteboard = NSPasteboard.general
    let snapshot = capturePasteboardSnapshot(pasteboard)
    pasteboard.clearContents()
    pasteboard.setString(text, forType: .string)
    let posted = synthesizeKeyPress(keyCode: 9, flags: .maskCommand)
    restorePasteboardSnapshot(pasteboard, snapshot: snapshot)
    return posted
}

private func typeTextViaEvents(_ text: String) -> Bool {
    let utf16 = Array(text.utf16)
    if utf16.isEmpty {
        return true
    }
    for chunk in stride(from: 0, to: utf16.count, by: 20) {
        let upper = min(chunk + 20, utf16.count)
        let slice = Array(utf16[chunk..<upper])
        guard let down = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: true),
              let up = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: false) else {
            return false
        }
        slice.withUnsafeBufferPointer { buffer in
            guard let base = buffer.baseAddress else { return }
            down.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: base)
            up.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: base)
        }
        down.post(tap: .cghidEventTap)
        usleep(8_000)
        up.post(tap: .cghidEventTap)
        usleep(12_000)
    }
    return true
}
