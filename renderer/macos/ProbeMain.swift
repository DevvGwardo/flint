// R0 feasibility helper only. No production entry point or interactive capability.
import AppKit
import WebKit
import Foundation
import Darwin

private let maximumInput = 512 * 1024
private let maximumSource = 256 * 1024
private let documentURL = URL(string: "flint-render://probe/document")!

private func readExactly(_ count: Int) -> Data? {
    var result = Data(count: count)
    var offset = 0
    let deadline = Date().addingTimeInterval(3)
    let valid = result.withUnsafeMutableBytes { buffer -> Bool in
        while offset < count && Date() < deadline {
            var descriptor = pollfd(fd: STDIN_FILENO, events: Int16(POLLIN), revents: 0)
            let ready = poll(&descriptor, 1, 100)
            if ready < 0 && errno != EINTR { return false }
            if ready <= 0 { continue }
            let length = Darwin.read(STDIN_FILENO, buffer.baseAddress!.advanced(by: offset), count - offset)
            if length <= 0 { return false }
            offset += length
        }
        return offset == count
    }
    return valid ? result : nil
}

private func writeFrame(_ bytes: Data) {
    guard bytes.count <= 8 * 1024 * 1024 else { exit(65) }
    var size = UInt32(bytes.count).bigEndian
    let header = withUnsafeBytes(of: &size) { Data($0) }
    for data in [header, bytes] {
        let okay = data.withUnsafeBytes { buffer -> Bool in
            var offset = 0
            let deadline = Date().addingTimeInterval(2)
            while offset < buffer.count && Date() < deadline {
                var descriptor = pollfd(fd: STDOUT_FILENO, events: Int16(POLLOUT), revents: 0)
                if poll(&descriptor, 1, 100) <= 0 { continue }
                let count = Darwin.write(STDOUT_FILENO, buffer.baseAddress!.advanced(by: offset), buffer.count - offset)
                if count <= 0 { return false }
                offset += count
            }
            return offset == buffer.count
        }
        if !okay { exit(66) }
    }
}

private func emit(_ value: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]),
          data.count <= 8192 else { exit(65) }
    writeFrame(data)
}

private struct Request: Decodable {
    let version: Int
    let revision: Int
    let source: String
    let fixture: String
    // Test-only values are synthetic run-root sentinel and local canary, never app resources.
    let sentinel: String
    let port: UInt16
}

private func nativeProbes(_ request: Request) {
    var limits = rlimit(rlim_cur: 0, rlim_max: 0)
    let coreResult = setrlimit(RLIMIT_CORE, &limits)
    let file = open(request.sentinel, O_RDONLY)
    let fileError = file < 0 ? errno : 0
    if file >= 0 { close(file) }
    let sock = socket(AF_INET, SOCK_STREAM, 0)
    var networkResult: Int32 = -1
    var networkError: Int32 = sock < 0 ? errno : 0
    if sock >= 0 {
        var address = sockaddr_in()
        address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        address.sin_family = sa_family_t(AF_INET)
        address.sin_port = request.port.bigEndian
        address.sin_addr.s_addr = inet_addr("127.0.0.1")
        networkResult = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(sock, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        networkError = networkResult < 0 ? errno : 0
        close(sock)
    }
    var descriptorCount = 0
    for descriptor in 3..<256 {
        if fcntl(Int32(descriptor), F_GETFD) >= 0 { descriptorCount += 1 }
    }
    let allowed = Set(["HOME", "TMPDIR", "PATH", "LANG", "LC_ALL"])
    emit(["event": "native", "revision": request.revision,
          "sentinelOpened": file >= 0, "fileErrno": fileError,
          "socketConnected": networkResult == 0, "networkErrno": networkError,
          "unexpectedEnvCount": ProcessInfo.processInfo.environment.keys.filter { !allowed.contains($0) }.count,
          "descriptorsAboveStdio": descriptorCount, "coreLimitExit": coreResult])
}

@MainActor
private final class Registry: NSObject, WKURLSchemeHandler {
    let source: Data
    var served = 0
    var rejected = 0
    var stopped = 0
    init(source: String) { self.source = Data(source.utf8) }
    func webView(_ webView: WKWebView, start task: any WKURLSchemeTask) {
        guard task.request.url?.absoluteString == documentURL.absoluteString,
              task.request.httpMethod == "GET", served == 0 else {
            rejected += 1
            task.didFailWithError(NSError(domain: "FlintProbe", code: 1))
            return
        }
        served += 1
        let policy = """
        default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src flint-render:; \
        font-src 'none'; connect-src 'none'; frame-src 'none'; worker-src 'none'; \
        object-src 'none'; media-src 'none'; base-uri 'none'; form-action 'none'
        """
        let response = HTTPURLResponse(url: documentURL, statusCode: 200, httpVersion: "HTTP/1.1",
                                       headerFields: ["Content-Type": "text/html; charset=utf-8",
                                                      "Content-Security-Policy": policy,
                                                      "Cache-Control": "no-store",
                                                      "X-Content-Type-Options": "nosniff"])!
        // Synchronous main-actor delivery: no queued completion survives stop(task).
        task.didReceive(response)
        task.didReceive(source)
        task.didFinish()
    }
    func webView(_ webView: WKWebView, stop task: any WKURLSchemeTask) { stopped += 1 }
}

@MainActor
private final class Host: NSObject, WKNavigationDelegate, WKUIDelegate, WKDownloadDelegate {
    let request: Request
    let registry: Registry
    var view: WKWebView!
    var window: NSWindow!
    var retired = false
    var firstNavigation = true
    var deniedNavigation = 0
    var deniedPopups = 0
    var deniedDialogs = 0
    var deniedPermissions = 0
    var deniedFiles = 0
    var deniedDownloads = 0
    var eofTimer: Timer?
    var deadlineTimer: Timer?
    var rulesStore: WKContentRuleListStore!

    init(_ request: Request) {
        self.request = request
        registry = Registry(source: request.source)
    }
    func finish(_ code: String, exitCode: Int32 = 0) {
        guard !retired else { return }
        retired = true
        eofTimer?.invalidate()
        deadlineTimer?.invalidate()
        view?.stopLoading()
        view?.navigationDelegate = nil
        view?.uiDelegate = nil
        window?.orderOut(nil)
        emit(["event": "closed", "code": code, "revision": request.revision,
              "deniedNavigation": deniedNavigation, "deniedPopups": deniedPopups,
              "deniedDialogs": deniedDialogs, "deniedPermissions": deniedPermissions,
              "deniedFiles": deniedFiles, "deniedDownloads": deniedDownloads,
              "registryServed": registry.served, "registryRejected": registry.rejected,
              "registryStopped": registry.stopped])
        exit(exitCode)
    }
    func start() {
        NSApplication.shared.setActivationPolicy(.prohibited)
        emit(["event": "appkit", "activationPolicy": NSApplication.shared.activationPolicy().rawValue,
              "isActive": NSApplication.shared.isActive, "revision": request.revision])
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.defaultWebpagePreferences.allowsContentJavaScript = false
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        configuration.allowsAirPlayForMediaPlayback = false
        configuration.mediaTypesRequiringUserActionForPlayback = .all
        configuration.userContentController.removeAllScriptMessageHandlers()
        configuration.setURLSchemeHandler(registry, forURLScheme: "flint-render")
        view = WKWebView(frame: NSRect(x: 0, y: 0, width: 128, height: 96), configuration: configuration)
        view.isInspectable = false
        view.navigationDelegate = self
        view.uiDelegate = self
        window = NSWindow(contentRect: NSRect(x: -10000, y: -10000, width: 128, height: 96),
                          styleMask: .borderless, backing: .buffered, defer: false)
        window.contentView = view
        // Never orderFront/makeKey/activate. This deliberately tests an ordered-out view.
        deadlineTimer = Timer.scheduledTimer(withTimeInterval: 8, repeats: false) { [weak self] _ in
            Task { @MainActor [weak self] in self?.finish("deadline", exitCode: 124) }
        }
        eofTimer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self, !self.retired else { return }
                var item = pollfd(fd: STDIN_FILENO, events: Int16(POLLIN), revents: 0)
                if poll(&item, 1, 0) > 0 && item.revents & Int16(POLLHUP | POLLIN) != 0 {
                    var byte: UInt8 = 0
                    let length = Darwin.read(STDIN_FILENO, &byte, 1)
                    self.finish(length == 0 ? "eof" : "cancel")
                }
            }
        }
        let scratch = ProcessInfo.processInfo.environment["TMPDIR"]!
        rulesStore = WKContentRuleListStore(url: URL(fileURLWithPath: scratch).appendingPathComponent("rules", isDirectory: true))
        let rules = """
        [{"trigger":{"url-filter":".*"},"action":{"type":"block"}},
         {"trigger":{"url-filter":"^flint-render://probe/document$"},"action":{"type":"ignore-previous-rules"}}]
        """
        rulesStore.compileContentRuleList(forIdentifier: "flint-r0", encodedContentRuleList: rules) { [weak self] rules, error in
            guard let self, !self.retired else { return }
            guard let rules, error == nil else { self.finish("rule_compile", exitCode: 70); return }
            self.view.configuration.userContentController.add(rules)
            emit(["event": "rules", "installed": true, "persistent": self.view.configuration.websiteDataStore.isPersistent,
                  "revision": self.request.revision])
            self.view.load(URLRequest(url: documentURL))
        }
    }
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
                 preferences: WKWebpagePreferences,
                 decisionHandler: @escaping (WKNavigationActionPolicy, WKWebpagePreferences) -> Void) {
        preferences.allowsContentJavaScript = false
        if firstNavigation && action.targetFrame?.isMainFrame == true && action.request.url == documentURL {
            firstNavigation = false
            decisionHandler(.allow, preferences)
        } else {
            deniedNavigation += 1
            decisionHandler(.cancel, preferences)
        }
    }
    func webView(_ webView: WKWebView, decidePolicyFor response: WKNavigationResponse,
                 decisionHandler: @escaping (WKNavigationResponsePolicy) -> Void) {
        decisionHandler(response.response.url == documentURL && response.isForMainFrame ? .allow : .cancel)
    }
    func webView(_ webView: WKWebView, didReceive challenge: URLAuthenticationChallenge,
                 completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        completionHandler(.cancelAuthenticationChallenge, nil)
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        guard !retired else { return }
        emit(["event": "loaded", "revision": request.revision, "orderedVisible": window.isVisible,
              "isActive": NSApplication.shared.isActive])
        if request.fixture == "hold" { return }
        runSyntheticScript()
    }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        finish("load_failed", exitCode: 71)
    }
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { finish("content_terminated", exitCode: 72) }
    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                 for action: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
        deniedPopups += 1
        return nil
    }
    func webView(_ webView: WKWebView, runJavaScriptAlertPanelWithMessage message: String,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping () -> Void) {
        deniedDialogs += 1
        completionHandler()
    }
    func webView(_ webView: WKWebView, runJavaScriptConfirmPanelWithMessage message: String,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping (Bool) -> Void) {
        deniedDialogs += 1
        completionHandler(false)
    }
    func webView(_ webView: WKWebView, runJavaScriptTextInputPanelWithPrompt prompt: String, defaultText: String?,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping (String?) -> Void) {
        deniedDialogs += 1
        completionHandler(nil)
    }
    func webView(_ webView: WKWebView, runOpenPanelWith parameters: WKOpenPanelParameters,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping ([URL]?) -> Void) {
        deniedFiles += 1
        completionHandler(nil)
    }
    func webView(_ webView: WKWebView, requestMediaCapturePermissionFor origin: WKSecurityOrigin,
                 initiatedByFrame frame: WKFrameInfo, type: WKMediaCaptureType,
                 decisionHandler: @escaping (WKPermissionDecision) -> Void) {
        deniedPermissions += 1
        decisionHandler(.deny)
    }
    func webView(_ webView: WKWebView, navigationAction: WKNavigationAction, didBecome download: WKDownload) {
        deniedDownloads += 1; download.delegate = self; download.cancel(nil)
    }
    func webView(_ webView: WKWebView, navigationResponse: WKNavigationResponse, didBecome download: WKDownload) {
        deniedDownloads += 1; download.delegate = self; download.cancel(nil)
    }
    func download(_ download: WKDownload, decideDestinationUsing response: URLResponse,
                  suggestedFilename: String, completionHandler: @escaping (URL?) -> Void) { completionHandler(nil) }

    func runSyntheticScript() {
        // Fixed synthetic attack corpus only; request source is never JS interpolation.
        let body = """
        const answer = {inlineScriptBlocked: !window.inlineExecuted,
                        handlersAbsent: !window.webkit?.messageHandlers ||
                          Object.keys(window.webkit.messageHandlers).length === 0,
                        secureContext: isSecureContext,
                        clipboardExposed: !!navigator.clipboard,
                        mediaDevicesExposed: !!navigator.mediaDevices,
                        geolocationExposed: !!navigator.geolocation,
                        notificationsExposed: typeof Notification !== 'undefined',
                        webauthnExposed: !!navigator.credentials,
                        paymentExposed: typeof PaymentRequest !== 'undefined',
                        filePickerExposed: typeof showOpenFilePicker !== 'undefined'};
        const attempts = [
          ['fetch', () => fetch('http://127.0.0.1:' + port + '/fetch')],
          ['fileFetch', () => fetch(sentinel)],
          ['websocket', () => new Promise((resolve, reject) => {
              let w = new WebSocket('ws://127.0.0.1:' + port + '/socket');
              w.onopen = () => {w.close(); resolve();}; w.onerror = reject;
          })],
          ['worker', () => {let w = new Worker('http://127.0.0.1:' + port + '/worker'); w.terminate();}],
          ['registryAlias', () => fetch('flint-render://probe/%64ocument')],
          ['registryUnknown', () => fetch('flint-render://probe/asset/not-registered')]
        ];
        for (const [name, action] of attempts) {
          try {
            await Promise.race([action(), new Promise((_, reject) => setTimeout(() => reject(0), 350))]);
            answer[name + 'Blocked'] = false;
          } catch {answer[name + 'Blocked'] = true;}
        }
        // Do not invoke permission APIs lacking a proven deny path: exposure is a blocker.
        const sheet = document.createElement('style');
        sheet.textContent = "body { background-image:url('http://127.0.0.1:" + port + "/css'); }";
        document.head.appendChild(sheet);
        const image = new Image(); image.src = 'http://127.0.0.1:' + port + '/image';
        document.body.appendChild(image);
        const script = document.createElement('script');
        script.textContent = 'window.dynamicExecuted=true'; document.body.appendChild(script);
        answer.dynamicScriptBlocked = !window.dynamicExecuted;
        answer.popupReturnedNull = window.open('http://127.0.0.1:' + port + '/popup') === null;
        const link = document.createElement('a');
        link.href = 'http://127.0.0.1:' + port + '/navigation'; link.click();
        return answer;
        """
        view.callAsyncJavaScript(body, arguments: ["port": request.port,
                                                  "sentinel": URL(fileURLWithPath: request.sentinel).absoluteString],
                                 in: nil, in: .page, completionHandler: { [weak self] result in
            guard let self, !self.retired else { return }
            switch result {
            case .success(let value):
                emit(["event": "webProbe", "revision": self.request.revision,
                      "checks": value as? [String: Any] ?? [:]])
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) { [weak self] in self?.snapshot() }
            case .failure:
                self.finish("script_probe", exitCode: 73)
            }
        })
    }
    func snapshot() {
        guard !retired else { return }
        let configuration = WKSnapshotConfiguration()
        configuration.rect = NSRect(x: 0, y: 0, width: 128, height: 96)
        configuration.snapshotWidth = 128
        view.takeSnapshot(with: configuration) { [weak self] image, error in
            guard let self, !self.retired else { return }
            guard error == nil, let image,
                  let cg = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
                self.finish("snapshot", exitCode: 74); return
            }
            let width = 128, height = 96
            guard width * height <= 2_000_000 else { self.finish("raster_limit", exitCode: 75); return }
            var bytes = Data(count: width * height * 4)
            let success = bytes.withUnsafeMutableBytes { storage -> Bool in
                let bitmap = CGBitmapInfo.byteOrder32Little.rawValue | CGImageAlphaInfo.premultipliedFirst.rawValue
                guard let context = CGContext(data: storage.baseAddress, width: width, height: height,
                                              bitsPerComponent: 8, bytesPerRow: width * 4,
                                              space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: bitmap) else { return false }
                context.draw(cg, in: CGRect(x: 0, y: 0, width: width, height: height))
                return true
            }
            guard success else { self.finish("bitmap", exitCode: 76); return }
            emit(["event": "raster", "revision": self.request.revision, "width": width, "height": height,
                  "format": "BGRA", "bytes": bytes.count, "snapshotWidth": cg.width, "snapshotHeight": cg.height])
            writeFrame(bytes)
            self.finish("complete")
        }
    }
}

@main
private struct ProbeMain {
    @MainActor static func main() {
        signal(SIGPIPE, SIG_IGN)
        guard let header = readExactly(4) else { emit(["event": "rejected", "code": "header"]); exit(64) }
        let count = header.reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
        guard count > 0, count <= maximumInput else { emit(["event": "rejected", "code": "input_limit"]); exit(64) }
        guard let body = readExactly(Int(count)), let request = try? JSONDecoder().decode(Request.self, from: body),
              request.version == 1, request.revision > 0, request.source.utf8.count <= maximumSource,
              ["raster", "hold"].contains(request.fixture),
              request.sentinel.hasPrefix("/Volumes/T7 Shield/mac-offload/flint-t3-implementation-glct94hg/renderer-evidence/"),
              request.port > 0 else { emit(["event": "rejected", "code": "request"]); exit(64) }
        nativeProbes(request)
        if ProcessInfo.processInfo.environment["TMPDIR"] == nil { exit(64) }
        let host = Host(request)
        host.start()
        withExtendedLifetime(host) { NSApplication.shared.run() }
    }
}
