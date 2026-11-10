// MethodChannel `nixxa/link` + EventChannel `nixxa/link/status`, the same
// API as the Android LinkChannel.kt (ARCHITECTURE.md channel table and
// "iOS (phase 3)" method mapping). Errors use the LinkException codes in
// mobile/lib/link/device_link.dart; DEVICE_ERROR carries the status byte.
import Flutter
import UIKit

final class NixxaLinkPlugin: NSObject, FlutterPlugin, FlutterStreamHandler {
  private let link = NixxaLink.shared
  private var listenerId: Int?

  static func register(with registrar: FlutterPluginRegistrar) {
    let instance = NixxaLinkPlugin()
    let methods = FlutterMethodChannel(name: "nixxa/link", binaryMessenger: registrar.messenger())
    registrar.addMethodCallDelegate(instance, channel: methods)
    let events = FlutterEventChannel(name: "nixxa/link/status", binaryMessenger: registrar.messenger())
    events.setStreamHandler(instance)
  }

  // MARK: FlutterStreamHandler

  func onListen(withArguments arguments: Any?, eventSink events: @escaping FlutterEventSink) -> FlutterError? {
    if let id = listenerId { link.removeListener(id) }
    listenerId = link.addListener { st in events(st.toMap()) }
    events(link.status.toMap())
    return nil
  }

  func onCancel(withArguments arguments: Any?) -> FlutterError? {
    if let id = listenerId { link.removeListener(id) }
    listenerId = nil
    return nil
  }

  // MARK: FlutterPlugin

  func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    let args = call.arguments as? [String: Any]
    switch call.method {
    case "status":
      result(link.status.toMap())

    case "pair":
      guard let qr = args?["qr"] as? String else {
        return result(FlutterError(code: "BAD_QR", message: "Missing QR text", details: nil))
      }
      link.pair(qr) { r in
        switch r {
        case .success(let st): result(st.toMap())
        case .failure(let e): result(NixxaLinkPlugin.flutterError(e))
        }
      }

    case "forget":
      link.forget { result(nil) }

    case "request":
      guard let op = (args?["op"] as? NSNumber)?.intValue, (0...0xFF).contains(op) else {
        return result(FlutterError(code: "DEVICE_ERROR", message: "Missing op", details: Proto.stBadMsg))
      }
      let payload = (args?["payload"] as? FlutterStandardTypedData)?.data ?? Data()
      link.request(UInt8(op), payload) { r in NixxaLinkPlugin.reply(result, r) }

    case "getNotifConfig":
      result(link.config.toMap())

    case "setNotifConfig":
      guard let m = args, let cfg = NotifConfig.fromMap(m) else {
        return result(FlutterError(code: "DEVICE_ERROR", message: "Bad config", details: Proto.stBadMsg))
      }
      link.setNotifConfig(cfg)
      result(nil)

    case "hasNotificationAccess":
      result(link.hasNotificationAccess)

    case "openNotificationAccessSettings":
      // iOS has no per-app notification access; the user turns on
      // "Share System Notifications" for the bunny in Settings > Bluetooth.
      if let url = URL(string: UIApplication.openSettingsURLString) {
        UIApplication.shared.open(url, options: [:], completionHandler: nil)
      }
      result(nil)

    case "requestBatteryExemption":
      result(nil) // nothing to ask for on iOS

    case "listApps":
      link.listApps { apps in
        result(apps.map { a -> [String: Any] in ["appId": a.appId, "label": a.label, "iconPng": NSNull()] })
      }

    case "sendTestNotification":
      link.sendTestNotification { r in
        switch r {
        case .success: result(nil)
        case .failure(let e): result(NixxaLinkPlugin.flutterError(e))
        }
      }

    default:
      result(FlutterMethodNotImplemented)
    }
  }

  private static func reply(_ result: FlutterResult, _ r: Result<Data, LinkError>) {
    switch r {
    case .success(let d): result(FlutterStandardTypedData(bytes: d))
    case .failure(let e): result(flutterError(e))
    }
  }

  private static func flutterError(_ e: LinkError) -> FlutterError {
    FlutterError(code: e.code, message: e.message, details: e.deviceStatus.map { NSNumber(value: $0) })
  }
}
