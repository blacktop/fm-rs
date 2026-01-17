// FoundationModels Swift FFI layer
// Provides C-compatible interface for Rust
//
// All @_cdecl functions use C calling convention and are safe to call from Rust.
// Memory management uses Unmanaged<AnyObject> for explicit retain/release.

import Foundation
import FoundationModels

// MARK: - Error Codes

private enum FFIErrorCode: Int32 {
    case unknown = 0
    case modelNotAvailable = 1
    case generationFailed = 2
    case cancelled = 3
    case toolError = 4
    case invalidInput = 5
    case timeout = 6
}

// MARK: - Callback Types

/// Tool callback type - Rust side implements this
/// Parameters: userData, toolName, argumentsJson
/// Returns: resultJson (Rust-allocated, caller must free with fm_rust_string_free)
private typealias ToolCallbackFn = @convention(c) (
    UnsafeMutableRawPointer?,
    UnsafePointer<CChar>?,
    UnsafePointer<CChar>?
) -> UnsafeMutablePointer<CChar>?

// MARK: - Error Handling

/// Error info container for FFI
private final class ErrorInfo: @unchecked Sendable {
    let message: String
    let code: Int32
    var toolName: String?
    var toolArguments: String?

    init(message: String, code: Int32, toolName: String? = nil, toolArguments: String? = nil) {
        self.message = message
        self.code = code
        self.toolName = toolName
        self.toolArguments = toolArguments
    }
}

/// Gets the error code from an error object.
@_cdecl("fm_error_code")
public func fm_error_code(_ errorPtr: UnsafeMutableRawPointer) -> Int32 {
    let errorInfo = Unmanaged<AnyObject>.fromOpaque(errorPtr).takeUnretainedValue() as! ErrorInfo
    return errorInfo.code
}

/// Gets the error message from an error object.
@_cdecl("fm_error_message")
public func fm_error_message(_ errorPtr: UnsafeMutableRawPointer) -> UnsafePointer<CChar>? {
    let errorInfo = Unmanaged<AnyObject>.fromOpaque(errorPtr).takeUnretainedValue() as! ErrorInfo
    return (errorInfo.message as NSString).utf8String
}

/// Gets the tool name from a tool error (may be null).
@_cdecl("fm_error_tool_name")
public func fm_error_tool_name(_ errorPtr: UnsafeMutableRawPointer) -> UnsafePointer<CChar>? {
    let errorInfo = Unmanaged<AnyObject>.fromOpaque(errorPtr).takeUnretainedValue() as! ErrorInfo
    guard let toolName = errorInfo.toolName else { return nil }
    return (toolName as NSString).utf8String
}

/// Gets the tool arguments JSON from a tool error (may be null).
@_cdecl("fm_error_tool_arguments")
public func fm_error_tool_arguments(_ errorPtr: UnsafeMutableRawPointer) -> UnsafePointer<CChar>? {
    let errorInfo = Unmanaged<AnyObject>.fromOpaque(errorPtr).takeUnretainedValue() as! ErrorInfo
    guard let args = errorInfo.toolArguments else { return nil }
    return (args as NSString).utf8String
}

/// Frees an error object.
@_cdecl("fm_error_free")
public func fm_error_free(_ errorPtr: UnsafeMutableRawPointer?) {
    guard let errorPtr = errorPtr else { return }
    Unmanaged<AnyObject>.fromOpaque(errorPtr).release()
}

/// Creates an error object.
private func createError(_ message: String, code: FFIErrorCode, toolName: String? = nil, toolArguments: String? = nil) -> UnsafeMutableRawPointer {
    let errorInfo = ErrorInfo(message: message, code: code.rawValue, toolName: toolName, toolArguments: toolArguments)
    return Unmanaged.passRetained(errorInfo as AnyObject).toOpaque()
}

/// Creates an error object from a Swift Error, extracting tool context if available.
private func createErrorFromException(_ error: Error, defaultCode: FFIErrorCode = .generationFailed) -> UnsafeMutableRawPointer {
    if let toolError = error as? ToolError {
        return createError(
            toolError.message,
            code: .toolError,
            toolName: toolError.toolName,
            toolArguments: toolError.toolArguments
        )
    } else if let timeoutError = error as? TimeoutError {
        return createError(timeoutError.message, code: .timeout)
    } else {
        return createError(error.localizedDescription, code: defaultCode)
    }
}

// MARK: - SystemLanguageModel

/// Creates the default SystemLanguageModel.
@_cdecl("fm_model_default")
public func fm_model_default(_ errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?) -> UnsafeMutableRawPointer? {
    let model = SystemLanguageModel.default
    return Unmanaged.passRetained(model as AnyObject).toOpaque()
}

/// Checks if the model is available.
@_cdecl("fm_model_is_available")
public func fm_model_is_available(_ modelPtr: UnsafeMutableRawPointer) -> Bool {
    let model = Unmanaged<AnyObject>.fromOpaque(modelPtr).takeUnretainedValue() as! SystemLanguageModel
    return model.isAvailable
}

/// Gets the availability status as an integer.
@_cdecl("fm_model_availability")
public func fm_model_availability(_ modelPtr: UnsafeMutableRawPointer) -> Int32 {
    let model = Unmanaged<AnyObject>.fromOpaque(modelPtr).takeUnretainedValue() as! SystemLanguageModel

    switch model.availability {
    case .available:
        return 0
    case .unavailable(.deviceNotEligible):
        return 1
    case .unavailable(.appleIntelligenceNotEnabled):
        return 2
    case .unavailable(.modelNotReady):
        return 3
    case .unavailable:
        return 4
    }
}

/// Frees a SystemLanguageModel.
@_cdecl("fm_model_free")
public func fm_model_free(_ modelPtr: UnsafeMutableRawPointer?) {
    guard let modelPtr = modelPtr else { return }
    Unmanaged<AnyObject>.fromOpaque(modelPtr).release()
}

// MARK: - Tool Definition from JSON

/// Tool definition parsed from Rust JSON
private struct ToolDefinitionDTO: Decodable {
    let name: String
    let description: String
    let argumentsSchemaJson: String

    enum CodingKeys: String, CodingKey {
        case name
        case description
        case argumentsSchema
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        name = try container.decode(String.self, forKey: .name)
        description = try container.decode(String.self, forKey: .description)
        // Decode argumentsSchema as raw JSON and re-encode to string for the description
        if let schemaValue = try? container.decode(AnyCodable.self, forKey: .argumentsSchema) {
            if let data = try? JSONEncoder().encode(schemaValue),
               let jsonStr = String(data: data, encoding: .utf8) {
                argumentsSchemaJson = jsonStr
            } else {
                argumentsSchemaJson = "{}"
            }
        } else {
            argumentsSchemaJson = "{}"
        }
    }
}

/// Helper for decoding arbitrary JSON values
private struct AnyCodable: Codable {
    let value: Any

    init(_ value: Any) {
        self.value = value
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            value = NSNull()
        } else if let bool = try? container.decode(Bool.self) {
            value = bool
        } else if let int = try? container.decode(Int.self) {
            value = int
        } else if let double = try? container.decode(Double.self) {
            value = double
        } else if let string = try? container.decode(String.self) {
            value = string
        } else if let array = try? container.decode([AnyCodable].self) {
            value = array.map { $0.value }
        } else if let dict = try? container.decode([String: AnyCodable].self) {
            value = dict.mapValues { $0.value }
        } else {
            throw DecodingError.dataCorruptedError(in: container, debugDescription: "Cannot decode value")
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch value {
        case is NSNull:
            try container.encodeNil()
        case let bool as Bool:
            try container.encode(bool)
        case let int as Int:
            try container.encode(int)
        case let double as Double:
            try container.encode(double)
        case let string as String:
            try container.encode(string)
        case let array as [Any]:
            try container.encode(array.map { AnyCodable($0) })
        case let dict as [String: Any]:
            try container.encode(dict.mapValues { AnyCodable($0) })
        default:
            throw EncodingError.invalidValue(value, EncodingError.Context(codingPath: container.codingPath, debugDescription: "Cannot encode value"))
        }
    }
}

private struct ToolError: Error, LocalizedError {
    let message: String
    let toolName: String?
    let toolArguments: String?

    init(message: String, toolName: String? = nil, toolArguments: String? = nil) {
        self.message = message
        self.toolName = toolName
        self.toolArguments = toolArguments
    }

    var errorDescription: String? { message }
}

private struct ToolResultDTO: Decodable {
    let success: Bool
    let content: String?
    let error: String?
}

// MARK: - Dynamic Tool Dispatcher

/// Stores tool context for callback dispatch
private final class ToolDispatcher: @unchecked Sendable {
    let toolDefinitions: [ToolDefinitionDTO]
    let userData: UnsafeMutableRawPointer?
    let callback: ToolCallbackFn

    init(toolDefinitions: [ToolDefinitionDTO], userData: UnsafeMutableRawPointer?, callback: @escaping ToolCallbackFn) {
        self.toolDefinitions = toolDefinitions
        self.userData = userData
        self.callback = callback
    }

    func callTool(name: String, argumentsJson: String) throws -> String {
        // Call Rust callback
        let resultPtr = name.withCString { namePtr in
            argumentsJson.withCString { argsPtr in
                callback(userData, namePtr, argsPtr)
            }
        }

        guard let resultPtr = resultPtr else {
            throw ToolError(message: "Tool callback returned null", toolName: name, toolArguments: argumentsJson)
        }

        // Parse result JSON
        let resultJson = String(cString: resultPtr)

        // Free the Rust-allocated string
        fm_rust_string_free(resultPtr)

        // Parse the result
        guard let resultData = resultJson.data(using: .utf8),
              let result = try? JSONDecoder().decode(ToolResultDTO.self, from: resultData) else {
            throw ToolError(message: "Failed to parse tool result JSON", toolName: name, toolArguments: argumentsJson)
        }

        if result.success {
            return result.content ?? ""
        } else {
            throw ToolError(message: result.error ?? "Unknown tool error", toolName: name, toolArguments: argumentsJson)
        }
    }
}

// MARK: - Generic Tool Bridge
//
// FoundationModels requires @Generable typed arguments for tools.
// We bridge dynamic Rust tools by creating a generic dispatcher tool that:
// 1. Accepts a tool name and JSON arguments from the model
// 2. Dispatches to the appropriate Rust callback
// 3. Returns the result to the model

/// Arguments for the generic tool dispatcher.
/// The model generates this structure to invoke a Rust-defined tool.
@Generable
private struct GenericToolArgument: Sendable, Codable {
    let name: String
    let value: String
}

@Generable
private struct GenericToolCallArguments: Sendable, Codable {
    /// The name of the tool to invoke (must match a registered Rust tool)
    let toolName: String

    /// Arguments for the tool as key/value pairs (values are JSON fragments)
    let arguments: [GenericToolArgument]

    enum CodingKeys: String, CodingKey {
        case toolName
        case toolNameSnake = "tool_name"
        case name
        case arguments
    }

    init(toolName: String, arguments: [GenericToolArgument]) {
        self.toolName = toolName
        self.arguments = arguments
    }

    init(from decoder: Decoder) throws {
        if let container = try? decoder.container(keyedBy: CodingKeys.self) {
            toolName = decodeToolName(from: container)
            arguments = (try? container.decode([GenericToolArgument].self, forKey: .arguments)) ?? []
            return
        }

        if let single = try? decoder.singleValueContainer() {
            if let text = try? single.decode(String.self) {
                toolName = text
                arguments = []
                return
            }
        }

        toolName = ""
        arguments = []
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(toolName, forKey: .toolName)
        try container.encode(arguments, forKey: .arguments)
    }
}

private func decodeToolName(
    from container: KeyedDecodingContainer<GenericToolCallArguments.CodingKeys>
) -> String {
    if let value = try? container.decode(String.self, forKey: .toolName) {
        return value
    }
    if let value = try? container.decode(String.self, forKey: .toolNameSnake) {
        return value
    }
    if let value = try? container.decode(String.self, forKey: .name) {
        return value
    }
    return ""
}

private func buildArgumentsJson(_ arguments: [GenericToolArgument]) -> String {
    var dict: [String: Any] = [:]
    for arg in arguments {
        if let parsed = parseJsonFragment(arg.value) {
            dict[arg.name] = parsed
        } else {
            dict[arg.name] = arg.value
        }
    }

    guard JSONSerialization.isValidJSONObject(dict),
          let data = try? JSONSerialization.data(withJSONObject: dict, options: []),
          let text = String(data: data, encoding: .utf8) else {
        return "{}"
    }

    return text
}

private func parseJsonFragment(_ text: String) -> Any? {
    guard let data = text.data(using: .utf8) else { return nil }
    return try? JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])
}

/// A bridge tool that dispatches to Rust-defined tools.
/// This is registered with FoundationModels and handles all dynamic tool calls.
private final class GenericToolBridge: Tool, @unchecked Sendable {
    typealias Arguments = GenericToolCallArguments
    typealias Output = String

    private let dispatcher: ToolDispatcher
    private let toolDescriptions: String

    /// Creates a bridge with the given dispatcher and tool descriptions for the schema.
    init(dispatcher: ToolDispatcher) {
        self.dispatcher = dispatcher
        // Build detailed description from tool definitions including schemas
        let descriptions = dispatcher.toolDefinitions.map { def in
            """
            Tool: \(def.name)
            Description: \(def.description)
            Arguments schema: \(def.argumentsSchemaJson)
            """
        }.joined(separator: "\n\n")
        self.toolDescriptions = descriptions
    }

    var name: String { "invoke_tool" }

    var description: String {
        """
        Invokes a registered tool by name. You must provide:
        1. toolName: The exact name of the tool to call
        2. arguments: A list of {name, value} pairs for the tool arguments

        IMPORTANT: The value field must be a JSON fragment (e.g., "42", "true", "\"hello\"", or "{\"key\": 1}") as a string.

        Available tools and their argument schemas:

        \(toolDescriptions)

        Example usage for a weather tool:
        - toolName: "checkWeather"
        - arguments: [ { "name": "location", "value": "Tokyo, Japan" } ]
        """
    }

    func call(arguments: GenericToolCallArguments) async throws -> String {
        let toolName = arguments.toolName.trimmingCharacters(in: .whitespacesAndNewlines)
        if toolName.isEmpty {
            throw ToolError(message: "Missing tool name")
        }
        let argsJson = buildArgumentsJson(arguments.arguments)
        return try dispatcher.callTool(name: toolName, argumentsJson: argsJson)
    }
}

/// Parses tool definitions from JSON
private func parseToolDefinitions(_ toolsJson: UnsafePointer<CChar>?) throws -> [ToolDefinitionDTO] {
    guard let toolsJson = toolsJson else { return [] }

    let jsonString = String(cString: toolsJson)
    guard let jsonData = jsonString.data(using: .utf8) else {
        throw ToolDefinitionError(message: "Invalid UTF-8 in tools JSON")
    }

    do {
        return try JSONDecoder().decode([ToolDefinitionDTO].self, from: jsonData)
    } catch {
        throw ToolDefinitionError(message: "Failed to decode tools JSON: \(error.localizedDescription)")
    }
}

private struct ToolDefinitionError: Error {
    let message: String
}

// MARK: - Session State

/// Session state container for FFI with tools support
private final class SessionState: @unchecked Sendable {
    let session: LanguageModelSession
    let toolDispatcher: ToolDispatcher?
    var currentTask: Task<Void, Never>?
    private let lock = NSLock()

    init(session: LanguageModelSession, toolDispatcher: ToolDispatcher? = nil) {
        self.session = session
        self.toolDispatcher = toolDispatcher
    }

    func setTask(_ task: Task<Void, Never>?) {
        lock.lock()
        defer { lock.unlock() }
        currentTask = task
    }

    func cancelCurrentTask() {
        lock.lock()
        defer { lock.unlock() }
        currentTask?.cancel()
        currentTask = nil
    }
}

// MARK: - Session Creation

/// Creates a new session with optional instructions, tools, and tool callback.
///
/// Tools are bridged through a GenericToolBridge that allows FoundationModels to
/// invoke Rust-defined tools dynamically. The model calls `invoke_tool` with the
/// tool name and JSON arguments, which dispatches to the appropriate Rust callback.
@_cdecl("fm_session_create")
public func fm_session_create(
    _ modelPtr: UnsafeMutableRawPointer,
    _ instructions: UnsafePointer<CChar>?,
    _ toolsJson: UnsafePointer<CChar>?,
    _ userData: UnsafeMutableRawPointer?,
    _ toolCallback: @escaping @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>?, UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>?,
    _ errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?
) -> UnsafeMutableRawPointer? {
    let model = Unmanaged<AnyObject>.fromOpaque(modelPtr).takeUnretainedValue() as! SystemLanguageModel

    // Parse instructions
    let instructionsValue: Instructions?
    if let instructions = instructions {
        instructionsValue = Instructions(String(cString: instructions))
    } else {
        instructionsValue = nil
    }

    // Parse tool definitions and create dispatcher + bridge
    let toolDefinitions: [ToolDefinitionDTO]
    do {
        toolDefinitions = try parseToolDefinitions(toolsJson)
    } catch let parseError as ToolDefinitionError {
        if let errorOut = errorOut {
            errorOut.pointee = createError(parseError.message, code: .invalidInput)
        }
        return nil
    } catch {
        if let errorOut = errorOut {
            errorOut.pointee = createError(
                "Failed to decode tools JSON: \(error.localizedDescription)",
                code: .invalidInput
            )
        }
        return nil
    }
    let toolDispatcher: ToolDispatcher?
    let toolBridge: GenericToolBridge?

    if !toolDefinitions.isEmpty {
        let dispatcher = ToolDispatcher(toolDefinitions: toolDefinitions, userData: userData, callback: toolCallback)
        toolDispatcher = dispatcher
        toolBridge = GenericToolBridge(dispatcher: dispatcher)
    } else {
        toolDispatcher = nil
        toolBridge = nil
    }

    // Create session with tool bridge if tools are defined
    let session: LanguageModelSession
    if let bridge = toolBridge {
        if let instructionsValue = instructionsValue {
            session = LanguageModelSession(model: model, tools: [bridge], instructions: instructionsValue)
        } else {
            session = LanguageModelSession(model: model, tools: [bridge])
        }
    } else {
        if let instructionsValue = instructionsValue {
            session = LanguageModelSession(model: model, instructions: instructionsValue)
        } else {
            session = LanguageModelSession(model: model)
        }
    }

    let state = SessionState(session: session, toolDispatcher: toolDispatcher)
    return Unmanaged.passRetained(state as AnyObject).toOpaque()
}

/// Creates a session from a transcript JSON string.
@_cdecl("fm_session_from_transcript")
public func fm_session_from_transcript(
    _ modelPtr: UnsafeMutableRawPointer,
    _ transcriptJson: UnsafePointer<CChar>,
    _ errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?
) -> UnsafeMutableRawPointer? {
    let model = Unmanaged<AnyObject>.fromOpaque(modelPtr).takeUnretainedValue() as! SystemLanguageModel
    let jsonString = String(cString: transcriptJson)

    guard let jsonData = jsonString.data(using: .utf8) else {
        if let errorOut = errorOut {
            errorOut.pointee = createError("Invalid UTF-8 in transcript JSON", code: .invalidInput)
        }
        return nil
    }

    do {
        let transcript = try JSONDecoder().decode(Transcript.self, from: jsonData)
        let session = LanguageModelSession(model: model, transcript: transcript)
        let state = SessionState(session: session)
        return Unmanaged.passRetained(state as AnyObject).toOpaque()
    } catch {
        if let errorOut = errorOut {
            errorOut.pointee = createError("Failed to decode transcript: \(error.localizedDescription)", code: .invalidInput)
        }
        return nil
    }
}

/// Frees a session.
@_cdecl("fm_session_free")
public func fm_session_free(_ sessionPtr: UnsafeMutableRawPointer?) {
    guard let sessionPtr = sessionPtr else { return }
    let state = Unmanaged<AnyObject>.fromOpaque(sessionPtr).takeRetainedValue() as! SessionState
    state.cancelCurrentTask()
}

// MARK: - Session Respond (Blocking)

/// Sends a prompt and blocks until response is ready.
@_cdecl("fm_session_respond")
public func fm_session_respond(
    _ sessionPtr: UnsafeMutableRawPointer,
    _ prompt: UnsafePointer<CChar>,
    _ optionsJson: UnsafePointer<CChar>?,
    _ errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?
) -> UnsafeMutablePointer<CChar>? {
    let state = Unmanaged<AnyObject>.fromOpaque(sessionPtr).takeUnretainedValue() as! SessionState
    let promptString = String(cString: prompt)
    let options = parseGenerationOptions(optionsJson)

    do {
        let content = try AsyncWaiter.wait {
            let response = try await state.session.respond(to: promptString, options: options)
            return response.content
        }

        return strdup(content)
    } catch {
        if let errorOut = errorOut {
            errorOut.pointee = createErrorFromException(error)
        }
        return nil
    }
}

/// Sends a prompt and blocks until response is ready, with a timeout in milliseconds.
@_cdecl("fm_session_respond_with_timeout")
public func fm_session_respond_with_timeout(
    _ sessionPtr: UnsafeMutableRawPointer,
    _ prompt: UnsafePointer<CChar>,
    _ optionsJson: UnsafePointer<CChar>?,
    _ timeoutMs: UInt64,
    _ errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?
) -> UnsafeMutablePointer<CChar>? {
    let state = Unmanaged<AnyObject>.fromOpaque(sessionPtr).takeUnretainedValue() as! SessionState
    let promptString = String(cString: prompt)
    let options = parseGenerationOptions(optionsJson)

    do {
        let content = try AsyncWaiter.wait(timeoutMs: timeoutMs) {
            let response = try await state.session.respond(to: promptString, options: options)
            return response.content
        }

        return strdup(content)
    } catch {
        if let errorOut = errorOut {
            errorOut.pointee = createErrorFromException(error)
        }
        return nil
    }
}

// MARK: - Session Streaming

/// Starts streaming a response.
///
/// Tool invocations during streaming are handled through the GenericToolBridge
/// registered during session creation, not through a separate callback.
@_cdecl("fm_session_stream")
public func fm_session_stream(
    _ sessionPtr: UnsafeMutableRawPointer,
    _ prompt: UnsafePointer<CChar>,
    _ optionsJson: UnsafePointer<CChar>?,
    _ userData: UnsafeMutableRawPointer?,
    _ onChunk: @escaping @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>?) -> Void,
    _ onDone: @escaping @convention(c) (UnsafeMutableRawPointer?) -> Void,
    _ onError: @escaping @convention(c) (UnsafeMutableRawPointer?, Int32, UnsafePointer<CChar>?) -> Void
) {
    let state = Unmanaged<AnyObject>.fromOpaque(sessionPtr).takeUnretainedValue() as! SessionState
    let promptString = String(cString: prompt)
    let options = parseGenerationOptions(optionsJson)

    let callbackQueue = DispatchQueue(label: "fm.ffi.callbacks", qos: .userInteractive)
    let semaphore = DispatchSemaphore(value: 0)

    let task = Task.detached {
        do {
            let stream = state.session.streamResponse(to: promptString, options: options)

            for try await partialResponse in stream {
                let content = partialResponse.content
                callbackQueue.sync {
                    content.withCString { ptr in
                        onChunk(userData, ptr)
                    }
                }

                if Task.isCancelled {
                    callbackQueue.sync {
                        "Cancelled".withCString { ptr in
                            onError(userData, FFIErrorCode.cancelled.rawValue, ptr)
                        }
                    }
                    semaphore.signal()
                    return
                }
            }

            callbackQueue.sync {
                onDone(userData)
            }
        } catch {
            callbackQueue.sync {
                // Extract tool error context if available
                let (errorCode, errorMessage): (Int32, String)
                if let toolError = error as? ToolError {
                    // Include tool context in the error message for streaming
                    var msg = toolError.message
                    if let name = toolError.toolName {
                        msg += " [tool: \(name)]"
                    }
                    errorCode = FFIErrorCode.toolError.rawValue
                    errorMessage = msg
                } else {
                    errorCode = FFIErrorCode.generationFailed.rawValue
                    errorMessage = error.localizedDescription
                }
                errorMessage.withCString { ptr in
                    onError(userData, errorCode, ptr)
                }
            }
        }

        semaphore.signal()
    }

    state.setTask(task)
    semaphore.wait()
    state.setTask(nil)
}

/// Cancels an ongoing stream operation.
@_cdecl("fm_session_cancel")
public func fm_session_cancel(_ sessionPtr: UnsafeMutableRawPointer) {
    let state = Unmanaged<AnyObject>.fromOpaque(sessionPtr).takeUnretainedValue() as! SessionState
    state.cancelCurrentTask()
}

/// Checks if the session is currently responding.
@_cdecl("fm_session_is_responding")
public func fm_session_is_responding(_ sessionPtr: UnsafeMutableRawPointer) -> Bool {
    let state = Unmanaged<AnyObject>.fromOpaque(sessionPtr).takeUnretainedValue() as! SessionState
    return state.session.isResponding
}

// MARK: - Transcript

/// Gets the session transcript as JSON.
@_cdecl("fm_session_get_transcript")
public func fm_session_get_transcript(
    _ sessionPtr: UnsafeMutableRawPointer,
    _ errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?
) -> UnsafeMutablePointer<CChar>? {
    let state = Unmanaged<AnyObject>.fromOpaque(sessionPtr).takeUnretainedValue() as! SessionState
    let transcript = state.session.transcript

    do {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let data = try encoder.encode(transcript)
        guard let jsonString = String(data: data, encoding: .utf8) else {
            if let errorOut = errorOut {
                errorOut.pointee = createError("Failed to encode transcript", code: .unknown)
            }
            return nil
        }
        return strdup(jsonString)
    } catch {
        if let errorOut = errorOut {
            errorOut.pointee = createError(
                "Failed to encode transcript: \(error.localizedDescription)",
                code: .unknown
            )
        }
        return nil
    }
}

/// Prewarms the model with an optional prompt prefix.
@_cdecl("fm_session_prewarm")
public func fm_session_prewarm(_ sessionPtr: UnsafeMutableRawPointer, _ promptPrefix: UnsafePointer<CChar>?) {
    let state = Unmanaged<AnyObject>.fromOpaque(sessionPtr).takeUnretainedValue() as! SessionState
    let prefix = promptPrefix.map { String(cString: $0) }

    Task.detached {
        if let prefix = prefix {
            state.session.prewarm(promptPrefix: Prompt(prefix))
        } else {
            state.session.prewarm()
        }
    }
}

// MARK: - Generation Options

/// Creates generation options from JSON.
@_cdecl("fm_generation_options_create")
public func fm_generation_options_create(_ optionsJson: UnsafePointer<CChar>?) -> UnsafeMutableRawPointer {
    let options = parseGenerationOptions(optionsJson)
    return Unmanaged.passRetained(options as AnyObject).toOpaque()
}

/// Frees generation options.
@_cdecl("fm_generation_options_free")
public func fm_generation_options_free(_ optionsPtr: UnsafeMutableRawPointer?) {
    guard let optionsPtr = optionsPtr else { return }
    Unmanaged<AnyObject>.fromOpaque(optionsPtr).release()
}

/// Parses generation options from JSON string.
private func parseGenerationOptions(_ optionsJson: UnsafePointer<CChar>?) -> GenerationOptions {
    guard let optionsJson = optionsJson else {
        return GenerationOptions()
    }

    let jsonString = String(cString: optionsJson)
    guard let jsonData = jsonString.data(using: .utf8) else {
        return GenerationOptions()
    }

    do {
        let decoded = try JSONDecoder().decode(GenerationOptionsDTO.self, from: jsonData)

        // Note: seed is not supported in current GenerationOptions API
        return GenerationOptions(
            sampling: decoded.sampling == "greedy" ? .greedy : nil,
            temperature: decoded.temperature,
            maximumResponseTokens: decoded.maximumResponseTokens.map { Int($0) }
        )
    } catch {
        return GenerationOptions()
    }
}

/// DTO for parsing generation options from JSON.
private struct GenerationOptionsDTO: Decodable {
    var temperature: Double?
    var sampling: String?
    var maximumResponseTokens: UInt32?
    var seed: UInt64?
}

// MARK: - Structured (JSON) Response

/// Sends a prompt and returns a JSON response matching the provided schema.
/// The schema is used to instruct the model to output valid JSON.
@_cdecl("fm_session_respond_json")
public func fm_session_respond_json(
    _ sessionPtr: UnsafeMutableRawPointer,
    _ prompt: UnsafePointer<CChar>,
    _ schemaJson: UnsafePointer<CChar>,
    _ optionsJson: UnsafePointer<CChar>?,
    _ errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?
) -> UnsafeMutablePointer<CChar>? {
    let state = Unmanaged<AnyObject>.fromOpaque(sessionPtr).takeUnretainedValue() as! SessionState
    let promptString = String(cString: prompt)
    let schemaString = String(cString: schemaJson)
    let options = parseGenerationOptions(optionsJson)

    // Build a prompt that instructs the model to output JSON matching the schema
    let structuredPrompt = """
    \(promptString)

    IMPORTANT: You must respond with valid JSON that matches this schema exactly:
    \(schemaString)

    Output only the JSON object, with no additional text, markdown formatting, or explanation.
    """

    do {
        let content = try AsyncWaiter.wait {
            let response = try await state.session.respond(to: structuredPrompt, options: options)
            return response.content
        }

        // Try to extract JSON from the response (handle potential markdown code blocks)
        let jsonContent = extractJson(from: content)
        return strdup(jsonContent)
    } catch {
        if let errorOut = errorOut {
            errorOut.pointee = createErrorFromException(error)
        }
        return nil
    }
}

/// Streams a JSON response matching the provided schema.
@_cdecl("fm_session_stream_json")
public func fm_session_stream_json(
    _ sessionPtr: UnsafeMutableRawPointer,
    _ prompt: UnsafePointer<CChar>,
    _ schemaJson: UnsafePointer<CChar>,
    _ optionsJson: UnsafePointer<CChar>?,
    _ userData: UnsafeMutableRawPointer?,
    _ onChunk: @escaping @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>?) -> Void,
    _ onDone: @escaping @convention(c) (UnsafeMutableRawPointer?) -> Void,
    _ onError: @escaping @convention(c) (UnsafeMutableRawPointer?, Int32, UnsafePointer<CChar>?) -> Void
) {
    let state = Unmanaged<AnyObject>.fromOpaque(sessionPtr).takeUnretainedValue() as! SessionState
    let promptString = String(cString: prompt)
    let schemaString = String(cString: schemaJson)
    let options = parseGenerationOptions(optionsJson)

    // Build a prompt that instructs the model to output JSON matching the schema
    let structuredPrompt = """
    \(promptString)

    IMPORTANT: You must respond with valid JSON that matches this schema exactly:
    \(schemaString)

    Output only the JSON object, with no additional text, markdown formatting, or explanation.
    """

    let callbackQueue = DispatchQueue(label: "fm.ffi.callbacks.json", qos: .userInteractive)
    let semaphore = DispatchSemaphore(value: 0)

    let task = Task.detached {
        do {
            let stream = state.session.streamResponse(to: structuredPrompt, options: options)

            for try await partialResponse in stream {
                let content = partialResponse.content
                callbackQueue.sync {
                    content.withCString { ptr in
                        onChunk(userData, ptr)
                    }
                }

                if Task.isCancelled {
                    callbackQueue.sync {
                        "Cancelled".withCString { ptr in
                            onError(userData, FFIErrorCode.cancelled.rawValue, ptr)
                        }
                    }
                    semaphore.signal()
                    return
                }
            }

            callbackQueue.sync {
                onDone(userData)
            }
        } catch {
            callbackQueue.sync {
                let errorCode = FFIErrorCode.generationFailed.rawValue
                let errorMessage = error.localizedDescription
                errorMessage.withCString { ptr in
                    onError(userData, errorCode, ptr)
                }
            }
        }

        semaphore.signal()
    }

    state.setTask(task)
    semaphore.wait()
    state.setTask(nil)
}

/// Extracts JSON from a response that might contain markdown code blocks or extra text.
private func extractJson(from content: String) -> String {
    let trimmed = content.trimmingCharacters(in: .whitespacesAndNewlines)

    // Check for markdown code block
    if trimmed.hasPrefix("```") {
        // Find the end of the opening fence
        if let startRange = trimmed.range(of: "\n") {
            let afterFence = trimmed[startRange.upperBound...]
            // Find closing fence
            if let endRange = afterFence.range(of: "```") {
                return String(afterFence[..<endRange.lowerBound]).trimmingCharacters(in: .whitespacesAndNewlines)
            }
        }
    }

    // Check if it starts with { or [ (already JSON) - extract balanced JSON
    if trimmed.hasPrefix("{") {
        return extractBalancedJson(from: trimmed, open: "{", close: "}")
    }
    if trimmed.hasPrefix("[") {
        return extractBalancedJson(from: trimmed, open: "[", close: "]")
    }

    // Try to find JSON object or array in the content - use whichever comes first
    let objectStart = trimmed.firstIndex(of: "{")
    let arrayStart = trimmed.firstIndex(of: "[")

    if let obj = objectStart, let arr = arrayStart {
        // Both found - use whichever comes first
        if obj <= arr {
            return extractBalancedJson(from: String(trimmed[obj...]), open: "{", close: "}")
        } else {
            return extractBalancedJson(from: String(trimmed[arr...]), open: "[", close: "]")
        }
    } else if let obj = objectStart {
        return extractBalancedJson(from: String(trimmed[obj...]), open: "{", close: "}")
    } else if let arr = arrayStart {
        return extractBalancedJson(from: String(trimmed[arr...]), open: "[", close: "]")
    }

    // Return as-is
    return trimmed
}

/// Extracts a balanced JSON structure (object or array) from the start of a string.
/// Handles nested structures and strings with escaped characters.
private func extractBalancedJson(from content: String, open: Character, close: Character) -> String {
    var depth = 0
    var inString = false
    var escapeNext = false
    var endIndex = content.startIndex

    for (index, char) in content.enumerated() {
        let stringIndex = content.index(content.startIndex, offsetBy: index)

        if escapeNext {
            escapeNext = false
            continue
        }

        if char == "\\" && inString {
            escapeNext = true
            continue
        }

        if char == "\"" {
            inString = !inString
            continue
        }

        if !inString {
            if char == open {
                depth += 1
            } else if char == close {
                depth -= 1
                if depth == 0 {
                    endIndex = content.index(after: stringIndex)
                    break
                }
            }
        }
    }

    // If we found a balanced structure, return it; otherwise return original
    if depth == 0 && endIndex > content.startIndex {
        return String(content[..<endIndex])
    }
    return content
}

// MARK: - String Helpers

/// Frees a string allocated by the Swift layer (strdup).
@_cdecl("fm_string_free")
public func fm_string_free(_ s: UnsafeMutablePointer<CChar>?) {
    guard let s = s else { return }
    free(s)
}

// fm_rust_string_free is exported by Rust (src/lib.rs) and linked at build time.
// Swift calls this to properly deallocate strings returned from Rust callbacks.
// Declaration for Swift to call the Rust-exported function:
@_silgen_name("fm_rust_string_free")
private func fm_rust_string_free(_ s: UnsafeMutablePointer<CChar>?)

// MARK: - Async Helpers

/// Helper for synchronously running Swift async code.
private final class AsyncWaiter {
    private final class AsyncState<T: Sendable>: @unchecked Sendable {
        var result: Result<T, Error>?
        let semaphore = DispatchSemaphore(value: 0)
    }

    static func wait<T: Sendable>(_ operation: @escaping @Sendable () async throws -> T) throws -> T {
        let state = AsyncState<T>()

        Task.detached {
            do {
                let value = try await operation()
                state.result = .success(value)
            } catch {
                state.result = .failure(error)
            }
            state.semaphore.signal()
        }

        state.semaphore.wait()

        switch state.result {
        case .success(let value):
            return value
        case .failure(let error):
            throw error
        case .none:
            throw TimeoutError(message: "No result available")
        }
    }

    static func wait<T: Sendable>(
        timeoutMs: UInt64,
        _ operation: @escaping @Sendable () async throws -> T
    ) throws -> T {
        let state = AsyncState<T>()
        let task = Task.detached {
            do {
                let value = try await operation()
                state.result = .success(value)
            } catch {
                state.result = .failure(error)
            }
            state.semaphore.signal()
        }

        let timeoutMsInt = timeoutMs > UInt64(Int.max) ? Int.max : Int(timeoutMs)
        if state.semaphore.wait(timeout: .now() + .milliseconds(timeoutMsInt)) == .timedOut {
            task.cancel()
            throw TimeoutError(message: "Timed out after \(timeoutMs) ms")
        }

        switch state.result {
        case .success(let value):
            return value
        case .failure(let error):
            throw error
        case .none:
            throw TimeoutError(message: "No result available")
        }
    }
}

private struct TimeoutError: Error {
    let message: String
}
