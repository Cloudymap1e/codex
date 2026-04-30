import DeviceCheck
import Foundation

struct DeviceCheckProbeReport: Encodable {
    let supported: Bool
    let tokenBase64: String?
    let error: String?
    let latencyMs: Double?
}

func writeReport(_ report: DeviceCheckProbeReport) throws {
    let data = try JSONEncoder().encode(report)
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data("\n".utf8))
}

let device = DCDevice.current
if !device.isSupported {
    let report = DeviceCheckProbeReport(
        supported: false,
        tokenBase64: nil,
        error: nil,
        latencyMs: nil
    )
    try writeReport(report)
    exit(0)
}

func requestToken() -> (result: DispatchTimeoutResult, token: Data?, error: Error?) {
    let semaphore = DispatchSemaphore(value: 0)
    var token: Data?
    var tokenError: Error?

    device.generateToken { data, error in
        token = data
        tokenError = error
        semaphore.signal()
    }

    return (semaphore.wait(timeout: .now() + 1), token, tokenError)
}

func isUnknownSystemFailure(_ error: Error?) -> Bool {
    (error as? DCError)?.code == .unknownSystemFailure
}

let tokenGenerationStart = DispatchTime.now()
var attempt = requestToken()
if attempt.result == .success, isUnknownSystemFailure(attempt.error) {
    attempt = requestToken()
}
let latencyMs = Double(
    DispatchTime.now().uptimeNanoseconds - tokenGenerationStart.uptimeNanoseconds
) / 1_000_000

if attempt.result == .timedOut {
    let report = DeviceCheckProbeReport(
        supported: true,
        tokenBase64: nil,
        error: "timed out waiting for DeviceCheck token",
        latencyMs: latencyMs
    )
    try writeReport(report)
    exit(1)
}

let report = DeviceCheckProbeReport(
    supported: true,
    tokenBase64: attempt.token?.base64EncodedString(),
    error: attempt.error.map(String.init(describing:)),
    latencyMs: latencyMs
)
try writeReport(report)
