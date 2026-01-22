"""Type stubs for the fm module (Apple FoundationModels.framework bindings)."""

from __future__ import annotations
from typing import Any, Callable, Optional
from enum import Enum

__version__: str
DEFAULT_CONTEXT_TOKENS: int

# Exceptions

class FmError(Exception):
    """Base exception for FoundationModels errors."""
    ...

class ModelNotAvailableError(FmError):
    """Model is not available on this device."""
    ...

class DeviceNotEligibleError(FmError):
    """Device is not eligible for Apple Intelligence."""
    ...

class AppleIntelligenceNotEnabledError(FmError):
    """Apple Intelligence is not enabled in system settings."""
    ...

class ModelNotReadyError(FmError):
    """Model is not ready (downloading or other system reasons)."""
    ...

class GenerationError(FmError):
    """Error during model generation."""
    ...

class ToolCallError(FmError):
    """Error during tool invocation."""
    ...

class JsonError(FmError):
    """JSON serialization/deserialization error."""
    ...

# Enums

class Sampling(Enum):
    """Sampling strategy for token generation."""
    Greedy = ...
    Random = ...

class ModelAvailability(Enum):
    """Represents the availability status of a FoundationModel."""
    Available = ...
    DeviceNotEligible = ...
    AppleIntelligenceNotEnabled = ...
    ModelNotReady = ...
    Unknown = ...

# Classes

class GenerationOptions:
    """Options that control how the model generates its response."""

    def __init__(
        self,
        *,
        temperature: Optional[float] = None,
        sampling: Optional[Sampling] = None,
        max_response_tokens: Optional[int] = None,
        seed: Optional[int] = None,
    ) -> None:
        """Creates a new GenerationOptions instance.

        Args:
            temperature: Temperature for sampling (0.0-2.0). Higher values produce more random outputs.
            sampling: Sampling strategy (Greedy or Random).
            max_response_tokens: Maximum number of tokens in the response.
            seed: Random seed for reproducible generation (currently not supported by Apple's API).
        """
        ...

    @property
    def temperature(self) -> Optional[float]: ...
    @property
    def sampling(self) -> Optional[Sampling]: ...
    @property
    def max_response_tokens(self) -> Optional[int]: ...
    @property
    def seed(self) -> Optional[int]: ...

class Response:
    """Response returned by the model."""

    @property
    def content(self) -> str:
        """Gets the text content of the response."""
        ...

    def __str__(self) -> str: ...
    def __len__(self) -> int: ...

class SystemLanguageModel:
    """The system language model provided by Apple Intelligence."""

    def __init__(self) -> None:
        """Creates the default system language model.

        Raises:
            ModelNotAvailableError: If the model cannot be created.
        """
        ...

    @property
    def is_available(self) -> bool:
        """Returns True if the model is available and ready to use."""
        ...

    @property
    def availability(self) -> ModelAvailability:
        """Gets the current availability status of the model."""
        ...

    def ensure_available(self) -> None:
        """Returns an error if the model is unavailable.

        Raises:
            DeviceNotEligibleError: If device is not eligible.
            AppleIntelligenceNotEnabledError: If Apple Intelligence is not enabled.
            ModelNotReadyError: If model is not ready.
            ModelNotAvailableError: If unavailable for unknown reason.
        """
        ...

class Session:
    """A session that interacts with a language model."""

    def __init__(
        self,
        model: SystemLanguageModel,
        *,
        instructions: Optional[str] = None,
        tools: Optional[list[Any]] = None,
    ) -> None:
        """Creates a new session with the given model.

        Args:
            model: The SystemLanguageModel to use.
            instructions: Optional instructions that define the model's behavior and role.
            tools: Optional list of tool objects. Each tool must have name, description,
                   arguments_schema attributes and a call(args) method.

        Raises:
            FmError: If session creation fails.
        """
        ...

    @staticmethod
    def from_transcript(model: SystemLanguageModel, transcript_json: str) -> Session:
        """Creates a session from a transcript JSON string.

        Args:
            model: The SystemLanguageModel to use.
            transcript_json: JSON string of the conversation transcript.

        Returns:
            A session restored from the transcript.
        """
        ...

    def respond(
        self,
        prompt: str,
        options: Optional[GenerationOptions] = None,
    ) -> Response:
        """Sends a prompt and waits for the complete response.

        Args:
            prompt: The text prompt to send.
            options: Optional generation options.

        Returns:
            The model's response.

        Raises:
            GenerationError: If generation fails.
        """
        ...

    def respond_with_timeout(
        self,
        prompt: str,
        timeout_secs: float,
        options: Optional[GenerationOptions] = None,
    ) -> Response:
        """Sends a prompt and waits for the complete response, with a timeout.

        Args:
            prompt: The text prompt to send.
            timeout_secs: Timeout in seconds.
            options: Optional generation options.

        Returns:
            The model's response.

        Raises:
            TimeoutError: If the operation times out.
            GenerationError: If generation fails.
        """
        ...

    def stream_response(
        self,
        prompt: str,
        on_chunk: Callable[[str], None],
        options: Optional[GenerationOptions] = None,
    ) -> None:
        """Sends a prompt and streams the response.

        Args:
            prompt: The text prompt to send.
            on_chunk: A callable that receives each text chunk.
            options: Optional generation options.

        Raises:
            GenerationError: If streaming fails.
        """
        ...

    def respond_structured(
        self,
        prompt: str,
        schema: dict[str, Any],
        options: Optional[GenerationOptions] = None,
    ) -> dict[str, Any]:
        """Sends a prompt and returns a structured JSON response.

        Args:
            prompt: The text prompt to send.
            schema: A JSON Schema dict describing the expected output format.
            options: Optional generation options.

        Returns:
            The parsed JSON response as a Python dict.

        Raises:
            GenerationError: If generation fails.
            JsonError: If JSON parsing fails.
        """
        ...

    def respond_json(
        self,
        prompt: str,
        schema: dict[str, Any],
        options: Optional[GenerationOptions] = None,
    ) -> str:
        """Sends a prompt and returns a raw JSON string response.

        Args:
            prompt: The text prompt to send.
            schema: A JSON Schema dict describing the expected output format.
            options: Optional generation options.

        Returns:
            The raw JSON string response.

        Raises:
            GenerationError: If generation fails.
        """
        ...

    def stream_json(
        self,
        prompt: str,
        schema: dict[str, Any],
        on_chunk: Callable[[str], None],
        options: Optional[GenerationOptions] = None,
    ) -> None:
        """Streams a structured JSON response.

        Args:
            prompt: The text prompt to send.
            schema: A JSON Schema dict describing the expected output format.
            on_chunk: A callable that receives each JSON chunk.
            options: Optional generation options.

        Raises:
            GenerationError: If streaming fails.
        """
        ...

    def cancel(self) -> None:
        """Cancels an ongoing stream operation."""
        ...

    @property
    def is_responding(self) -> bool:
        """Returns True if the session is currently generating a response."""
        ...

    @property
    def transcript_json(self) -> str:
        """Gets the session transcript as a JSON string."""
        ...

    def context_usage(self, limit: ContextLimit) -> ContextUsage:
        """Estimates current context usage based on the session transcript.

        Args:
            limit: The context limit configuration.

        Returns:
            The estimated context usage.
        """
        ...

    def prewarm(self, prompt_prefix: Optional[str] = None) -> None:
        """Prewarms the model with an optional prompt prefix.

        Args:
            prompt_prefix: Optional text to prewarm with.
        """
        ...

class ToolOutput:
    """Output returned by a tool invocation."""

    content: str

    def __init__(self, content: str) -> None:
        """Creates a new tool output with the given content."""
        ...

class ContextLimit:
    """Configuration for estimating context usage."""

    def __init__(
        self,
        max_tokens: int,
        *,
        reserved_response_tokens: int = 0,
        chars_per_token: int = 4,
    ) -> None:
        """Creates a new context limit with a max token budget.

        Args:
            max_tokens: Maximum tokens available in the session context window.
            reserved_response_tokens: Tokens reserved for the model's next response.
            chars_per_token: Estimated characters per token (English ~3-4, CJK ~1).
        """
        ...

    @staticmethod
    def default_on_device() -> ContextLimit:
        """Creates a default configuration for on-device models.

        Returns:
            Default limit with 4096 tokens, 512 reserved.
        """
        ...

    @property
    def max_tokens(self) -> int: ...
    @property
    def reserved_response_tokens(self) -> int: ...
    @property
    def chars_per_token(self) -> int: ...

class ContextUsage:
    """Estimated context usage for a session."""

    estimated_tokens: int
    max_tokens: int
    reserved_response_tokens: int
    available_tokens: int
    utilization: float
    over_limit: bool

class Schema:
    """A fluent JSON Schema builder for structured generation."""

    def __init__(self) -> None:
        """Creates a new empty schema."""
        ...

    @staticmethod
    def string() -> Schema:
        """Creates a string schema."""
        ...

    @staticmethod
    def integer() -> Schema:
        """Creates an integer schema."""
        ...

    @staticmethod
    def number() -> Schema:
        """Creates a number schema."""
        ...

    @staticmethod
    def boolean() -> Schema:
        """Creates a boolean schema."""
        ...

    @staticmethod
    def null() -> Schema:
        """Creates a null schema."""
        ...

    @staticmethod
    def object() -> Schema:
        """Creates an object schema."""
        ...

    @staticmethod
    def array(items: Optional[Schema] = None) -> Schema:
        """Creates an array schema.

        Args:
            items: Optional schema for array items.
        """
        ...

    def property(
        self,
        name: str,
        schema: Schema,
        *,
        required: bool = False,
    ) -> Schema:
        """Adds a property to an object schema.

        Args:
            name: Property name.
            schema: Property schema.
            required: Whether the property is required.

        Returns:
            Self for chaining.
        """
        ...

    def description(self, description: str) -> Schema:
        """Sets a description for this schema."""
        ...

    def minimum(self, value: float) -> Schema:
        """Sets the minimum value for a number/integer schema."""
        ...

    def maximum(self, value: float) -> Schema:
        """Sets the maximum value for a number/integer schema."""
        ...

    def min_length(self, value: int) -> Schema:
        """Sets the minimum length for a string schema."""
        ...

    def max_length(self, value: int) -> Schema:
        """Sets the maximum length for a string schema."""
        ...

    def pattern(self, pattern: str) -> Schema:
        """Sets a regex pattern for a string schema."""
        ...

    def enum_(self, values: list[str]) -> Schema:
        """Sets allowed enum values."""
        ...

    def default(self, value: str) -> Schema:
        """Sets the default value."""
        ...

    def min_items(self, value: int) -> Schema:
        """Sets the minimum number of items for an array schema."""
        ...

    def max_items(self, value: int) -> Schema:
        """Sets the maximum number of items for an array schema."""
        ...

    def unique_items(self, value: bool) -> Schema:
        """Sets whether array items must be unique."""
        ...

    def no_additional_properties(self) -> Schema:
        """Disallows additional properties in an object schema."""
        ...

    def to_dict(self) -> dict[str, Any]:
        """Converts the schema to a Python dict."""
        ...

    def to_json(self) -> str:
        """Converts the schema to a JSON string."""
        ...

# Functions

def estimate_tokens(text: str, chars_per_token: int = 4) -> int:
    """Estimates tokens based on a characters-per-token heuristic.

    Args:
        text: The text to estimate tokens for.
        chars_per_token: Estimated characters per token (default: 4).

    Returns:
        Estimated number of tokens.
    """
    ...

def context_usage_from_transcript(
    transcript_json: str,
    limit: ContextLimit,
) -> ContextUsage:
    """Estimates token usage for a session transcript JSON.

    Args:
        transcript_json: The transcript JSON string.
        limit: The context limit configuration.

    Returns:
        The estimated context usage.
    """
    ...

def transcript_to_text(transcript_json: str) -> str:
    """Extracts readable text from transcript JSON.

    Args:
        transcript_json: The transcript JSON string.

    Returns:
        The extracted text.
    """
    ...

def compact_transcript(
    model: SystemLanguageModel,
    transcript_json: str,
    *,
    chunk_tokens: int = 800,
    max_summary_tokens: int = 400,
) -> str:
    """Compacts a transcript into a summary using the on-device model.

    Args:
        model: The SystemLanguageModel to use.
        transcript_json: The transcript JSON string to compact.
        chunk_tokens: Estimated tokens per chunk (default: 800).
        max_summary_tokens: Maximum tokens for the summary (default: 400).

    Returns:
        The compacted summary.
    """
    ...
