import Foundation

final class DoubaoBridge {
    private let center = DistributedNotificationCenter.default()

    func requestSnapshots(timeout: TimeInterval) throws -> [SettingsSnapshot] {
        var snapshots = Set<SettingsSnapshot>()
        let observer = center.addObserver(
            forName: Notification.Name(NotificationName.respondSnapshot),
            object: nil,
            queue: nil
        ) { note in
            guard let info = note.userInfo else { return }
            let requestId = info["requestId"] as? String ?? ""
            let rawSettings = info["settings"] as? [String: Any] ?? [:]
            let normalized = rawSettings
                .mapValues { String(describing: $0) }
            snapshots.insert(SettingsSnapshot(requestId: requestId, settings: normalized))
        }

        defer { center.removeObserver(observer) }
        center.postNotificationName(
            Notification.Name(NotificationName.requestSnapshot),
            object: nil,
            userInfo: [:],
            deliverImmediately: true
        )
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.1))
        }
        if snapshots.isEmpty {
            throw CLIError.timeout("no snapshot response within \(timeout)s")
        }
        return snapshots.sorted { lhs, rhs in
            let left = lhs.settings["asrShortcutKeyDisplay"] ?? ""
            let right = rhs.settings["asrShortcutKeyDisplay"] ?? ""
            return "\(lhs.requestId)|\(left)" < "\(rhs.requestId)|\(right)"
        }
    }

    func postShortcutConfig(
        startEnabled: Bool,
        globalEnabled: Bool,
        keyCode: Int,
        modifierFlags: Int,
        display: String
    ) {
        center.postNotificationName(
            Notification.Name(NotificationName.enableStartASRShortcut),
            object: nil,
            userInfo: ["enable": startEnabled],
            deliverImmediately: true
        )
        center.postNotificationName(
            Notification.Name(NotificationName.enableGlobalASRShortcut),
            object: nil,
            userInfo: ["enable": globalEnabled],
            deliverImmediately: true
        )
        center.postNotificationName(
            Notification.Name(NotificationName.asrShortcutKey),
            object: nil,
            userInfo: [
                "keyCode": keyCode,
                "modifierFlags": modifierFlags,
                "display": display,
            ],
            deliverImmediately: true
        )
    }

    func startMicrophone(deviceId: String) {
        center.postNotificationName(
            Notification.Name(NotificationName.selectedMicrophoneId),
            object: nil,
            userInfo: ["selectedMicrophoneId": deviceId],
            deliverImmediately: true
        )
        center.postNotificationName(
            Notification.Name(NotificationName.startMicrophoneMonitor),
            object: nil,
            userInfo: ["id": deviceId],
            deliverImmediately: true
        )
    }

    func stopMicrophone(deviceId: String? = nil) {
        center.postNotificationName(
            Notification.Name(NotificationName.stopMicrophoneMonitor),
            object: nil,
            userInfo: deviceId.map { ["id": $0] },
            deliverImmediately: true
        )
    }
}
