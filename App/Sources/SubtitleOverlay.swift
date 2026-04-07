import AppKit
import Foundation

private let subtitleHideDelay: TimeInterval = 0.6
private let subtitleBottomMargin: CGFloat = 34
private let subtitleWidthRatio: CGFloat = 0.42
private let subtitleMinWidth: CGFloat = 124
private let subtitleMaxWidth: CGFloat = 560
private let subtitleCapsuleHeight: CGFloat = 42
private let subtitleHorizontalPadding: CGFloat = 12
private let subtitleWaveWidth: CGFloat = 26
private let subtitleWaveHeight: CGFloat = 18
private let subtitleWaveBarCount = 5
private let subtitleWaveBarWidth: CGFloat = 2.2
private let subtitleWaveBarGap: CGFloat = 2.4
private let subtitleWaveMinFraction: CGFloat = 0.04
private let subtitleWaveAttackFactor = 0.92
private let subtitleWaveReleaseFactor = 0.42
private let subtitleWaveSilenceLevel = 0.045
private let subtitleContentGap: CGFloat = 10
private let subtitleFontSize: CGFloat = 12
private let subtitleLabelHeight: CGFloat = 15
private let subtitleLineHeight: CGFloat = 15
private let subtitleTextWidthFactor: CGFloat = 1.04
private let subtitleWrapUnitFactor: CGFloat = 0.78
private let subtitleMaxVisibleLines = 4
private let subtitleRenderCadence: TimeInterval = 1.0 / 60.0

private struct SubtitleSessionState {
    var committedText = ""
    var liveText = ""
}

private struct SubtitleTranscriptSnapshot {
    let displayText: String
    let commitText: String
}

private enum SubtitleTheme {
    case light
    case dark
}

private func subtitleIsSentenceBreak(_ ch: Character) -> Bool {
    matches(ch, ["。", "！", "？", "!", "?", "；", ";"])
}

private func subtitleJoinText(_ prefix: String, _ suffix: String) -> String {
    let left = prefix.trimmingCharacters(in: .whitespacesAndNewlines)
    let right = suffix.trimmingCharacters(in: .whitespacesAndNewlines)
    if left.isEmpty { return right }
    if right.isEmpty { return left }
    let needSpace = left.last.map(subtitleIsASCIIAlnum) == true && right.first.map(subtitleIsASCIIAlnum) == true
    return needSpace ? "\(left) \(right)" : "\(left)\(right)"
}

private func subtitleStateFullText(_ state: SubtitleSessionState) -> String {
    subtitleJoinText(state.committedText, state.liveText)
}

private func subtitleDisplayText(_ text: String) -> String {
    let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
    if trimmed.isEmpty { return "" }
    var lines: [String] = []
    var current = ""
    for ch in trimmed {
        current.append(ch)
        if subtitleIsSentenceBreak(ch) {
            let line = current.trimmingCharacters(in: .whitespacesAndNewlines)
            if !line.isEmpty {
                lines.append(line)
            }
            current.removeAll(keepingCapacity: true)
        }
    }
    let tail = current.trimmingCharacters(in: .whitespacesAndNewlines)
    if !tail.isEmpty {
        lines.append(tail)
    }
    if lines.isEmpty {
        return trimmed
    }
    return lines.suffix(subtitleMaxVisibleLines).reduce(into: "") { partial, line in
        partial = subtitleJoinText(partial, line)
    }
}

private func subtitleSplitCommittedTail(_ text: String) -> (String, String) {
    let normalized = text.trimmingCharacters(in: .whitespacesAndNewlines)
    if normalized.isEmpty { return ("", "") }
    var lastBreakEnd: String.Index?
    var index = normalized.startIndex
    while index < normalized.endIndex {
        let next = normalized.index(after: index)
        if subtitleIsSentenceBreak(normalized[index]) {
            lastBreakEnd = next
        }
        index = next
    }
    guard let end = lastBreakEnd else {
        return ("", normalized)
    }
    let committed = normalized[..<end].trimmingCharacters(in: .whitespacesAndNewlines)
    let tail = normalized[end...].trimmingCharacters(in: .whitespacesAndNewlines)
    return (committed, tail)
}

private func subtitleStripSentenceBreaks(_ text: String) -> String {
    text.filter { !subtitleIsSentenceBreak($0) }
}

private func subtitleSharedPrefixChars(_ lhs: String, _ rhs: String) -> Int {
    var count = 0
    var left = lhs.makeIterator()
    var right = rhs.makeIterator()
    while let a = left.next(), let b = right.next(), a == b {
        count += 1
    }
    return count
}

private func subtitlePrefixLooksLikeFullTranscript(_ prefix: String, _ full: String) -> Bool {
    let prefixTrimmed = prefix.trimmingCharacters(in: .whitespacesAndNewlines)
    let fullTrimmed = full.trimmingCharacters(in: .whitespacesAndNewlines)
    if prefixTrimmed.isEmpty || fullTrimmed.isEmpty {
        return false
    }
    if fullTrimmed.hasPrefix(prefixTrimmed) {
        return true
    }
    let prefixStripped = subtitleStripSentenceBreaks(prefixTrimmed)
    let fullStripped = subtitleStripSentenceBreaks(fullTrimmed)
    if !prefixStripped.isEmpty && fullStripped.hasPrefix(prefixStripped) {
        return true
    }
    let shared = subtitleSharedPrefixChars(prefixStripped, fullStripped)
    let prefixCount = prefixStripped.count
    let fullCount = fullStripped.count
    return shared >= 3 && shared * 2 >= max(2, min(prefixCount, fullCount))
}

private func subtitleSessionSnapshot(_ state: SubtitleSessionState) -> SubtitleTranscriptSnapshot {
    let commitText = subtitleStateFullText(state)
    return SubtitleTranscriptSnapshot(
        displayText: subtitleDisplayText(commitText),
        commitText: commitText
    )
}

private func subtitleStateApplyPartial(_ state: inout SubtitleSessionState, _ normalized: String) {
    if normalized.isEmpty { return }
    let previousFull = subtitleStateFullText(state)
    let checkCommitted = subtitlePrefixLooksLikeFullTranscript(state.committedText, normalized)
    let checkPrevious = subtitlePrefixLooksLikeFullTranscript(previousFull, normalized)
    let prefersFullTranscript = state.committedText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || checkCommitted || checkPrevious
    let nextFull = prefersFullTranscript ? normalized : subtitleJoinText(state.committedText, normalized)
    let next = subtitleSplitCommittedTail(nextFull)
    state.committedText = next.0
    state.liveText = next.1
}

private func subtitleSessionApplyPartial(_ state: inout SubtitleSessionState, _ text: String) -> SubtitleTranscriptSnapshot {
    let normalized = text.trimmingCharacters(in: .whitespacesAndNewlines)
    if normalized.isEmpty {
        return subtitleSessionSnapshot(state)
    }
    subtitleStateApplyPartial(&state, normalized)
    return subtitleSessionSnapshot(state)
}

private func subtitleStateApplyFinal(_ state: inout SubtitleSessionState, _ normalized: String) {
    let resolved = normalized.isEmpty ? subtitleStateFullText(state) : normalized
    let next = subtitleSplitCommittedTail(resolved)
    state.committedText = next.0
    state.liveText = next.1
}

private func subtitleSessionApplyFinal(_ state: inout SubtitleSessionState, _ text: String) -> SubtitleTranscriptSnapshot {
    let normalized = text.trimmingCharacters(in: .whitespacesAndNewlines)
    subtitleStateApplyFinal(&state, normalized)
    return subtitleSessionSnapshot(state)
}

private func subtitleTextUnits(_ text: String) -> CGFloat {
    let units = text.reduce(CGFloat(0)) { partial, ch in
        if ch.isWhitespace {
            return partial + 0.35
        }
        if ch.isASCII {
            return partial + 0.58
        }
        return partial + 1
    }
    return max(1, units)
}

private func subtitleWrappedLineCount(_ text: String, labelWidth: CGFloat) -> Int {
    let unitsPerLine = max(6, labelWidth / (subtitleFontSize * subtitleWrapUnitFactor))
    let lines = text.split(separator: "\n", omittingEmptySubsequences: false).reduce(0) { partial, rawLine in
        let units = subtitleTextUnits(String(rawLine))
        let wrapped = Int(ceil(units / unitsPerLine))
        return partial + max(1, wrapped)
    }
    return max(1, min(subtitleMaxVisibleLines, lines))
}

private func subtitleWindowFrame(for text: String, screen: NSScreen) -> NSRect {
    let visible = screen.visibleFrame
    let textUnits = subtitleTextUnits(text)
    let preferredTextWidth = max(84, (textUnits + 1.2) * (subtitleFontSize * subtitleTextWidthFactor))
    var width = preferredTextWidth + subtitleHorizontalPadding * 2 + subtitleWaveWidth + subtitleContentGap
    width = max(width, subtitleMinWidth)
    let maxAvailableWidth = max(200, visible.width - 40)
    let maxWidth = min(subtitleMaxWidth, max(subtitleMinWidth, visible.width * subtitleWidthRatio), maxAvailableWidth)
    width = min(max(width, subtitleMinWidth), maxWidth)

    let labelWidth = max(40, width - subtitleHorizontalPadding * 2 - subtitleWaveWidth - subtitleContentGap)
    let lineCount = subtitleWrappedLineCount(text, labelWidth: labelWidth)
    let labelHeight = max(subtitleLabelHeight, CGFloat(lineCount) * subtitleLineHeight)
    let height = max(subtitleCapsuleHeight, labelHeight + 14)
    let x = visible.minX + max(0, (visible.width - width) / 2)
    let y = visible.minY + subtitleBottomMargin
    return NSRect(x: x, y: y, width: width, height: height)
}

private func subtitleWaveformOnlyFrame(screen: NSScreen) -> NSRect {
    let visible = screen.visibleFrame
    let size = subtitleCapsuleHeight
    let x = visible.minX + max(0, (visible.width - size) / 2)
    let y = visible.minY + subtitleBottomMargin
    return NSRect(x: x, y: y, width: size, height: size)
}

private func subtitleWaveformFrame(for frame: NSRect) -> NSRect {
    let x: CGFloat
    if frame.width <= subtitleCapsuleHeight * 1.2 {
        x = (frame.width - subtitleWaveWidth) / 2
    } else {
        x = subtitleHorizontalPadding
    }
    return NSRect(
        x: x,
        y: max(0, (frame.height - subtitleWaveHeight) / 2),
        width: subtitleWaveWidth,
        height: subtitleWaveHeight
    )
}

private func subtitleLabelFrame(for frame: NSRect, text: String) -> NSRect {
    let labelWidth = max(40, frame.width - subtitleHorizontalPadding * 2 - subtitleWaveWidth - subtitleContentGap)
    let lineCount = subtitleWrappedLineCount(text, labelWidth: labelWidth)
    let labelHeight = min(frame.height, max(subtitleLabelHeight, CGFloat(lineCount) * subtitleLineHeight))
    return NSRect(
        x: subtitleHorizontalPadding + subtitleWaveWidth + subtitleContentGap,
        y: max(0, (frame.height - labelHeight) / 2),
        width: labelWidth,
        height: labelHeight
    )
}

private func subtitleCornerRadius(_ frame: NSRect) -> CGFloat {
    min(22, max(18, frame.height * 0.5))
}

private func subtitleRectsClose(_ lhs: NSRect, _ rhs: NSRect) -> Bool {
    abs(lhs.origin.x - rhs.origin.x) < 0.5
        && abs(lhs.origin.y - rhs.origin.y) < 0.5
        && abs(lhs.size.width - rhs.size.width) < 0.5
        && abs(lhs.size.height - rhs.size.height) < 0.5
}

private func subtitleCurrentTheme() -> SubtitleTheme {
    let best = NSApp.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua])
    return best == .darkAqua ? .dark : .light
}

private func subtitleIsASCIIAlnum(_ ch: Character) -> Bool {
    ch.unicodeScalars.allSatisfy { $0.isASCII && CharacterSet.alphanumerics.contains($0) }
}

private func matches<T: Equatable>(_ value: T, _ items: [T]) -> Bool {
    items.contains(value)
}

private extension Character {
    var isWhitespace: Bool {
        unicodeScalars.allSatisfy(CharacterSet.whitespacesAndNewlines.contains)
    }

    var isASCII: Bool {
        unicodeScalars.allSatisfy(\.isASCII)
    }
}

final class SubtitleOverlayController {
    private let panel: NSPanel
    private let effectView: NSVisualEffectView
    private let contentView: NSView
    private let waveformView: NSView
    private let textField: NSTextField
    private let waveBars: [NSView]

    private var visible = false
    private var smoothedLevel = 0.0
    private var wavePhase = 0.0
    private var hideWorkItem: DispatchWorkItem?
    private var session = SubtitleSessionState()
    private var pendingLevel: Double?
    private var pendingPartialText: String?
    private var renderScheduled = false
    private var lastRenderAt: TimeInterval = 0
    private var lastDisplayText = ""
    private var themedFrameHeight: CGFloat = 0
    private var themedStyle: (theme: SubtitleTheme, reduceTransparency: Bool)?

    init() {
        let frame = NSRect(x: 0, y: 0, width: subtitleMinWidth, height: subtitleCapsuleHeight)
        panel = NSPanel(
            contentRect: frame,
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.isFloatingPanel = true
        panel.level = .statusBar
        panel.backgroundColor = .clear
        panel.isOpaque = false
        panel.hasShadow = false
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient, .ignoresCycle]
        panel.ignoresMouseEvents = true

        effectView = NSVisualEffectView(frame: frame)
        effectView.autoresizingMask = [.width, .height]
        effectView.material = .hudWindow
        effectView.blendingMode = .behindWindow
        effectView.state = .active
        effectView.wantsLayer = true

        contentView = NSView(frame: frame)
        contentView.autoresizingMask = [.width, .height]
        contentView.wantsLayer = true

        waveformView = NSView(frame: subtitleWaveformFrame(for: frame))
        waveformView.wantsLayer = true

        var bars: [NSView] = []
        for _ in 0..<subtitleWaveBarCount {
            let bar = NSView(frame: .zero)
            bar.wantsLayer = true
            waveformView.addSubview(bar)
            bars.append(bar)
        }
        waveBars = bars

        textField = NSTextField(labelWithString: "")
        textField.font = NSFont.systemFont(ofSize: subtitleFontSize, weight: .medium)
        textField.alignment = .left
        textField.lineBreakMode = .byWordWrapping
        textField.maximumNumberOfLines = subtitleMaxVisibleLines
        textField.backgroundColor = .clear
        textField.isBezeled = false
        textField.isBordered = false
        textField.drawsBackground = false

        contentView.addSubview(waveformView)
        contentView.addSubview(textField)
        effectView.addSubview(contentView)
        panel.contentView = effectView
        applyLayout(frame: frame, text: "", forceTheme: true)
        panel.orderOut(nil)
    }

    func resetSession() {
        guard Thread.isMainThread else {
            DispatchQueue.main.async { [weak self] in self?.resetSession() }
            return
        }
        session = SubtitleSessionState()
        pendingPartialText = nil
    }

    func showWaveformOnly() {
        guard Thread.isMainThread else {
            DispatchQueue.main.async { [weak self] in self?.showWaveformOnly() }
            return
        }
        hideWorkItem?.cancel()
        session = SubtitleSessionState()
        smoothedLevel = 0
        wavePhase = 0
        pendingLevel = nil
        pendingPartialText = nil
        renderScheduled = false
        lastDisplayText = ""
        textField.stringValue = ""
        let frame = currentScreen().map(subtitleWaveformOnlyFrame) ?? panel.frame
        applyVisible(frame: frame, text: "", forceTheme: true)
        RuntimeTimeline.shared.record("overlay", "show_waveform_only")
        updateWaveform(level: 0, frame: frame)
    }

    func updateLevel(_ level: Double) {
        guard Thread.isMainThread else {
            DispatchQueue.main.async { [weak self] in self?.updateLevel(level) }
            return
        }
        guard visible else { return }
        hideWorkItem?.cancel()
        pendingLevel = level
        scheduleRenderIfNeeded()
    }

    func showPartial(_ text: String) {
        guard Thread.isMainThread else {
            DispatchQueue.main.async { [weak self] in self?.showPartial(text) }
            return
        }
        hideWorkItem?.cancel()
        pendingPartialText = text
        scheduleRenderIfNeeded()
    }

    @discardableResult
    func showFinal(_ text: String) -> String {
        guard Thread.isMainThread else {
            DispatchQueue.main.async { [weak self] in
                _ = self?.showFinal(text)
            }
            return text.trimmingCharacters(in: .whitespacesAndNewlines)
        }
        pendingPartialText = nil
        renderScheduled = false
        let snapshot = subtitleSessionApplyFinal(&session, text)
        guard !snapshot.commitText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              !snapshot.displayText.isEmpty else {
            hide()
            return ""
        }
        hideWorkItem?.cancel()
        let screen = currentScreen() ?? NSScreen.screens.first
        let frame = screen.map { subtitleWindowFrame(for: snapshot.displayText, screen: $0) } ?? panel.frame
        applyVisible(frame: frame, text: snapshot.displayText)
        RuntimeTimeline.shared.record("overlay", "final_render", fields: [
            "display": runtimeTimelineTextFields(snapshot.displayText),
            "commit": runtimeTimelineTextFields(snapshot.commitText),
        ])
        updateWaveform(level: smoothedLevel, frame: panel.frame)
        let work = DispatchWorkItem { [weak self] in
            self?.hide()
        }
        hideWorkItem = work
        DispatchQueue.main.asyncAfter(deadline: .now() + subtitleHideDelay, execute: work)
        return snapshot.commitText
    }

    func hide() {
        guard Thread.isMainThread else {
            DispatchQueue.main.async { [weak self] in self?.hide() }
            return
        }
        hideWorkItem?.cancel()
        hideWorkItem = nil
        session = SubtitleSessionState()
        smoothedLevel = 0
        wavePhase = 0
        pendingLevel = nil
        pendingPartialText = nil
        renderScheduled = false
        lastDisplayText = ""
        visible = false
        panel.orderOut(nil)
        RuntimeTimeline.shared.record("overlay", "hide")
    }

    private func currentScreen() -> NSScreen? {
        NSScreen.main ?? NSScreen.screens.first
    }

    private func scheduleRenderIfNeeded() {
        guard pendingPartialText != nil || (visible && pendingLevel != nil) else { return }
        let now = CFAbsoluteTimeGetCurrent()
        let elapsed = now - lastRenderAt
        if !renderScheduled && elapsed >= subtitleRenderCadence {
            flushPendingRender()
            return
        }
        guard !renderScheduled else { return }
        renderScheduled = true
        let delay = max(0, subtitleRenderCadence - elapsed)
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
            self?.flushPendingRender()
        }
    }

    private func flushPendingRender() {
        guard Thread.isMainThread else {
            DispatchQueue.main.async { [weak self] in self?.flushPendingRender() }
            return
        }
        renderScheduled = false
        lastRenderAt = CFAbsoluteTimeGetCurrent()

        if let text = pendingPartialText {
            pendingPartialText = nil
            let snapshot = subtitleSessionApplyPartial(&session, text)
            if !snapshot.displayText.isEmpty {
                let changed = snapshot.displayText != lastDisplayText
                let screen = currentScreen() ?? NSScreen.screens.first
                let frame = screen.map { subtitleWindowFrame(for: snapshot.displayText, screen: $0) } ?? panel.frame
                applyVisible(frame: frame, text: snapshot.displayText)
                if changed {
                    RuntimeTimeline.shared.record("overlay", "partial_render", fields: runtimeTimelineTextFields(snapshot.displayText))
                }
            }
        }

        if let level = pendingLevel, visible {
            pendingLevel = nil
            applyLevel(level)
            updateWaveform(level: smoothedLevel, frame: panel.frame)
        }
    }

    private func applyVisible(frame: NSRect, text: String, forceTheme: Bool = false) {
        let firstShow = !visible
        visible = true
        applyLayout(frame: frame, text: text, forceTheme: forceTheme || firstShow)
        if firstShow {
            panel.alphaValue = 1
            panel.orderFrontRegardless()
        }
    }

    private func applyLevel(_ level: Double) {
        let current = smoothedLevel
        let next: Double
        if level <= subtitleWaveSilenceLevel {
            next = current * 0.42
        } else {
            let factor = level > current ? subtitleWaveAttackFactor : subtitleWaveReleaseFactor
            next = current + (level - current) * factor
        }
        smoothedLevel = next < 0.01 ? 0 : min(max(next, 0), 1)
        if smoothedLevel > 0 {
            wavePhase += 0.70 + smoothedLevel * 1.15
        }
    }

    private func applyLayout(frame: NSRect, text: String, forceTheme: Bool) {
        let frameChanged = !subtitleRectsClose(panel.frame, frame)
        if frameChanged {
            panel.setFrame(frame, display: false)
            effectView.frame = NSRect(origin: .zero, size: frame.size)
            contentView.frame = effectView.bounds
            waveformView.frame = subtitleWaveformFrame(for: frame)
        }
        let labelFrame = subtitleLabelFrame(for: frame, text: text)
        if frameChanged || !subtitleRectsClose(textField.frame, labelFrame) {
            textField.frame = labelFrame
        }
        if lastDisplayText != text {
            textField.stringValue = text
            lastDisplayText = text
        }
        applyTheme(frame: frame, force: forceTheme || frameChanged)
    }

    private func applyTheme(frame: NSRect, force: Bool) {
        let theme = subtitleCurrentTheme()
        let reduceTransparency = NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency
        let radius = subtitleCornerRadius(frame)
        let styleChanged =
            themedStyle?.theme != theme
            || themedStyle?.reduceTransparency != reduceTransparency
            || abs(themedFrameHeight - frame.height) >= 0.5
        guard force || styleChanged else { return }
        let fillColor: NSColor
        let borderColor: NSColor
        let shadowColor: NSColor
        let textColor: NSColor
        let evenBarColor: NSColor
        let oddBarColor: NSColor
        if reduceTransparency {
            effectView.material = .hudWindow
            effectView.state = .active
            switch theme {
            case .light:
                fillColor = NSColor(calibratedRed: 0.97, green: 0.97, blue: 0.955, alpha: 0.80)
                borderColor = NSColor(calibratedWhite: 1.0, alpha: 0.12)
                shadowColor = NSColor(calibratedWhite: 0.0, alpha: 0.05)
                textColor = NSColor(calibratedWhite: 0.18, alpha: 0.78)
                evenBarColor = NSColor(calibratedRed: 0.15, green: 0.45, blue: 0.95, alpha: 0.82)
                oddBarColor = NSColor(calibratedRed: 0.15, green: 0.45, blue: 0.95, alpha: 0.42)
            case .dark:
                fillColor = NSColor(calibratedRed: 0.12, green: 0.13, blue: 0.15, alpha: 0.76)
                borderColor = NSColor(calibratedWhite: 1.0, alpha: 0.08)
                shadowColor = NSColor(calibratedWhite: 0.0, alpha: 0.14)
                textColor = NSColor(calibratedWhite: 0.96, alpha: 0.82)
                evenBarColor = NSColor(calibratedRed: 0.40, green: 0.68, blue: 1.0, alpha: 0.86)
                oddBarColor = NSColor(calibratedRed: 0.40, green: 0.68, blue: 1.0, alpha: 0.48)
            }
        } else {
            effectView.material = .hudWindow
            effectView.state = .active
            switch theme {
            case .light:
                fillColor = NSColor(calibratedWhite: 1.0, alpha: 0.52)
                borderColor = NSColor(calibratedWhite: 0.0, alpha: 0.06)
                shadowColor = NSColor(calibratedWhite: 0.0, alpha: 0.10)
                textColor = NSColor(calibratedWhite: 0.10, alpha: 0.88)
                evenBarColor = NSColor(calibratedRed: 0.15, green: 0.45, blue: 0.95, alpha: 0.86)
                oddBarColor = NSColor(calibratedRed: 0.15, green: 0.45, blue: 0.95, alpha: 0.44)
            case .dark:
                fillColor = NSColor(calibratedWhite: 0.04, alpha: 0.48)
                borderColor = NSColor(calibratedWhite: 1.0, alpha: 0.08)
                shadowColor = NSColor(calibratedWhite: 0.0, alpha: 0.14)
                textColor = NSColor(calibratedWhite: 0.97, alpha: 0.90)
                evenBarColor = NSColor(calibratedRed: 0.42, green: 0.70, blue: 1.0, alpha: 0.88)
                oddBarColor = NSColor(calibratedRed: 0.42, green: 0.70, blue: 1.0, alpha: 0.50)
            }
        }

        effectView.layer?.cornerRadius = radius
        effectView.layer?.masksToBounds = true
        textField.textColor = textColor
        contentView.layer?.backgroundColor = fillColor.cgColor
        contentView.layer?.cornerRadius = radius
        contentView.layer?.borderWidth = 0.55
        contentView.layer?.borderColor = borderColor.cgColor
        contentView.layer?.shadowColor = shadowColor.cgColor
        contentView.layer?.shadowOpacity = theme == .dark ? 0.32 : 0.22
        contentView.layer?.shadowRadius = theme == .dark ? 14 : 12
        for (index, bar) in waveBars.enumerated() {
            bar.layer?.backgroundColor = (index % 2 == 0 ? evenBarColor : oddBarColor).cgColor
            bar.layer?.cornerRadius = subtitleWaveBarWidth / 2
        }
        themedStyle = (theme, reduceTransparency)
        themedFrameHeight = frame.height
    }

    private func updateWaveform(level: Double, frame: NSRect) {
        let waveFrame = subtitleWaveformFrame(for: frame)
        waveformView.frame = waveFrame
        let barWidths = Array(repeating: subtitleWaveBarWidth, count: waveBars.count)
        let totalWidth = barWidths.reduce(0, +) + subtitleWaveBarGap * CGFloat(max(0, waveBars.count - 1))
        var x = max(0, (waveFrame.width - totalWidth) / 2)
        let energy = min(max(CGFloat(level), 0), 1)
        let idlePattern: [CGFloat] = [0.16, 0.30, 0.22, 0.40, 0.16]
        let profile: [CGFloat] = [0.30, 0.72, 0.48, 0.86, 0.28]
        let offsets: [CGFloat] = [0.0, 1.257, 2.513, 0.628, 1.885]
        for (index, bar) in waveBars.enumerated() {
            let width = barWidths[index]
            let fraction: CGFloat
            if energy <= 0.001 {
                fraction = max(idlePattern[index], subtitleWaveMinFraction)
            } else {
                let motion = min(max(0.26 + energy * 1.10, 0.26), 1)
                let pulse = pow((sin(CGFloat(wavePhase) * 1.75 + offsets[index]) * 0.5 + 0.5), 0.66)
                let sway = pow((sin(CGFloat(wavePhase) * 2.45 + offsets[index] * 1.12) * 0.5 + 0.5), 0.90)
                let bounce = pow((cos(CGFloat(wavePhase) * 3.10 + offsets[index] * 0.54) * 0.5 + 0.5), 1.08)
                let base = idlePattern[index] * (0.50 + motion * 0.24)
                let lift = motion * profile[index] * (0.26 + 0.88 * pulse)
                let accent = motion * (0.14 * sway + 0.10 * bounce)
                fraction = min(max(base + lift + accent, subtitleWaveMinFraction), 0.96)
            }
            let height = min(waveFrame.height, max(6, waveFrame.height * fraction))
            let y = (waveFrame.height - height) / 2
            bar.frame = NSRect(x: x, y: y, width: width, height: height)
            x += width + subtitleWaveBarGap
        }
    }
}
