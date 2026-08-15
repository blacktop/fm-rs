import Foundation
import FoundationModels

@available(iOS 27.0, macOS 27.0, visionOS 27.0, *)
private func reasoningLevel(from rawValue: Int32) -> ContextOptions.ReasoningLevel? {
    switch rawValue {
    case 1:
        return .light
    case 2:
        return .moderate
    case 3:
        return .deep
    default:
        return nil
    }
}

private func respondWithReasoning(
    sessionPtr: UnsafeMutableRawPointer,
    prompt: UnsafePointer<CChar>,
    optionsJson: UnsafePointer<CChar>?,
    reasoningLevelValue: Int32,
    timeoutMs: UInt64?,
    errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?
) -> UnsafeMutablePointer<CChar>? {
    guard #available(iOS 27.0, macOS 27.0, visionOS 27.0, *) else {
        errorOut?.pointee = createError(
            "Extended reasoning requires macOS 27.0, iOS 27.0, visionOS 27.0, or later",
            code: .unsupportedPlatform
        )
        return nil
    }

    guard let level = reasoningLevel(from: reasoningLevelValue) else {
        errorOut?.pointee = createError(
            "Reasoning level must be light, moderate, or deep",
            code: .invalidInput
        )
        return nil
    }

    let state = Unmanaged<AnyObject>.fromOpaque(sessionPtr).takeUnretainedValue() as! SessionState
    let promptString = String(cString: prompt)
    let options = parseGenerationOptions(optionsJson)
    let contextOptions = ContextOptions(reasoningLevel: level)
    let operation: @Sendable () async throws -> (String, String?) = {
        let response = try await state.session.respond(
            to: promptString,
            options: options,
            contextOptions: contextOptions
        )
        return (response.content, responseUsageJson(response))
    }

    do {
        let (content, usageJson) = if let timeoutMs {
            try AsyncWaiter.wait(session: state, timeoutMs: timeoutMs, operation)
        } else {
            try AsyncWaiter.wait(session: state, operation)
        }
        state.setLastResponseUsage(usageJson)
        return strdup(content)
    } catch {
        errorOut?.pointee = createGenerationErrorFromException(error)
        return nil
    }
}

/// Responds with the Foundation Models 27 extended-reasoning context option.
@_cdecl("fm_session_respond_with_reasoning")
public func fm_session_respond_with_reasoning(
    _ sessionPtr: UnsafeMutableRawPointer,
    _ prompt: UnsafePointer<CChar>,
    _ optionsJson: UnsafePointer<CChar>?,
    _ reasoningLevelValue: Int32,
    _ errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?
) -> UnsafeMutablePointer<CChar>? {
    respondWithReasoning(
        sessionPtr: sessionPtr,
        prompt: prompt,
        optionsJson: optionsJson,
        reasoningLevelValue: reasoningLevelValue,
        timeoutMs: nil,
        errorOut: errorOut
    )
}

/// Responds with extended reasoning, bounded by a timeout in milliseconds.
@_cdecl("fm_session_respond_with_reasoning_timeout")
public func fm_session_respond_with_reasoning_timeout(
    _ sessionPtr: UnsafeMutableRawPointer,
    _ prompt: UnsafePointer<CChar>,
    _ optionsJson: UnsafePointer<CChar>?,
    _ reasoningLevelValue: Int32,
    _ timeoutMs: UInt64,
    _ errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?
) -> UnsafeMutablePointer<CChar>? {
    respondWithReasoning(
        sessionPtr: sessionPtr,
        prompt: prompt,
        optionsJson: optionsJson,
        reasoningLevelValue: reasoningLevelValue,
        timeoutMs: timeoutMs,
        errorOut: errorOut
    )
}
