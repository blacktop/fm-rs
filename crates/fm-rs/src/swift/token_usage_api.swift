import Foundation
import FoundationModels

private struct TokenUsageError: Error, LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

private func noOpToolCallback(
    _ userData: UnsafeMutableRawPointer?,
    _ toolName: UnsafePointer<CChar>?,
    _ argumentsJson: UnsafePointer<CChar>?
) -> UnsafeMutablePointer<CChar>? {
    _ = userData
    _ = toolName
    _ = argumentsJson
    return nil
}

private func tokenUsageBridge(from toolsJson: UnsafePointer<CChar>?) throws -> GenericToolBridge? {
    let toolDefinitions = try parseToolDefinitions(toolsJson)
    if toolDefinitions.isEmpty {
        return nil
    }
    let dispatcher = ToolDispatcher(
        toolDefinitions: toolDefinitions,
        userData: nil,
        callback: noOpToolCallback
    )
    return GenericToolBridge(dispatcher: dispatcher)
}

/// Returns token usage for a prompt using 26.4+ APIs when available.
/// Returns a sentinel when runtime APIs are unavailable.
@_cdecl("fm_model_token_usage_for")
public func fm_model_token_usage_for(
    _ modelPtr: UnsafeMutableRawPointer,
    _ prompt: UnsafePointer<CChar>,
    _ errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?
) -> Int64 {
    let model = Unmanaged<AnyObject>.fromOpaque(modelPtr).takeUnretainedValue() as! SystemLanguageModel
    let promptString = String(cString: prompt)

    if #available(iOS 26.4, macOS 26.4, visionOS 26.4, *) {
        do {
            let count = try AsyncWaiter.wait {
                try await model.tokenCount(for: promptString)
            }
            guard let tokenCount = Int64(exactly: count) else {
                throw TokenUsageError(message: "Token count value is out of Int64 range")
            }
            return tokenCount
        } catch {
            if let errorOut = errorOut {
                errorOut.pointee = createGenerationErrorFromException(error)
            }
            return -1
        }
    }

    // Runtime is older than 26.4; Rust will use local token estimation fallback.
    _ = errorOut
    return tokenUsageUnavailableSentinel
}

/// Returns token usage for instructions + tools using 26.4+ APIs when available.
/// Returns a sentinel when runtime APIs are unavailable.
@_cdecl("fm_model_token_usage_for_tools")
public func fm_model_token_usage_for_tools(
    _ modelPtr: UnsafeMutableRawPointer,
    _ instructions: UnsafePointer<CChar>,
    _ toolsJson: UnsafePointer<CChar>?,
    _ errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?
) -> Int64 {
    let model = Unmanaged<AnyObject>.fromOpaque(modelPtr).takeUnretainedValue() as! SystemLanguageModel
    let instructionsString = String(cString: instructions)

    if #available(iOS 26.4, macOS 26.4, visionOS 26.4, *) {
        do {
            let bridge = try tokenUsageBridge(from: toolsJson)
            let tools: [any Tool] = bridge.map { [$0] } ?? []
            let count = try AsyncWaiter.wait {
                try await model.tokenCount(for: Instructions(instructionsString))
            }
            guard let tokenCount = Int64(exactly: count) else {
                throw TokenUsageError(message: "Token count value is out of Int64 range")
            }
            return tokenCount
        } catch {
            if let errorOut = errorOut {
                errorOut.pointee = createGenerationErrorFromException(error)
            }
            return -1
        }
    }

    // Runtime is older than 26.4; Rust will use local token estimation fallback.
    _ = errorOut
    return tokenUsageUnavailableSentinel
}
