"""Basic tests for the fm module.

These tests verify the Python bindings work correctly.
Tests marked with @pytest.mark.integration require Apple Intelligence
and should be run with: pytest -m integration
"""

import pytest


def test_import():
    """Test that the module can be imported."""
    import fm

    assert hasattr(fm, "SystemLanguageModel")
    assert hasattr(fm, "Session")
    assert hasattr(fm, "GenerationOptions")
    assert hasattr(fm, "Response")
    assert hasattr(fm, "Schema")
    assert hasattr(fm, "__version__")


def test_version():
    """Test that __version__ is set."""
    import fm

    assert fm.__version__
    assert isinstance(fm.__version__, str)


def test_default_context_tokens():
    """Test that DEFAULT_CONTEXT_TOKENS is set."""
    import fm

    assert fm.DEFAULT_CONTEXT_TOKENS == 4096


def test_sampling_enum():
    """Test Sampling enum values."""
    import fm

    assert fm.Sampling.Greedy is not None
    assert fm.Sampling.Random is not None
    assert fm.Sampling.Greedy != fm.Sampling.Random


def test_model_availability_enum():
    """Test ModelAvailability enum values."""
    import fm

    assert fm.ModelAvailability.Available is not None
    assert fm.ModelAvailability.DeviceNotEligible is not None
    assert fm.ModelAvailability.AppleIntelligenceNotEnabled is not None
    assert fm.ModelAvailability.ModelNotReady is not None
    assert fm.ModelAvailability.Unknown is not None


def test_generation_options():
    """Test GenerationOptions creation."""
    import fm

    # Default options
    opts = fm.GenerationOptions()
    assert opts.temperature is None
    assert opts.sampling is None
    assert opts.max_response_tokens is None

    # With parameters
    opts = fm.GenerationOptions(
        temperature=0.7,
        sampling=fm.Sampling.Random,
        max_response_tokens=500,
    )
    assert opts.temperature == 0.7
    assert opts.sampling == fm.Sampling.Random
    assert opts.max_response_tokens == 500


def test_generation_options_temperature_validation():
    """Test that temperature validation works."""
    import fm

    # Valid temperature
    opts = fm.GenerationOptions(temperature=1.5)
    assert opts.temperature == 1.5

    # Invalid temperature should raise
    with pytest.raises(ValueError, match="Temperature must be between"):
        fm.GenerationOptions(temperature=3.0)

    with pytest.raises(ValueError, match="Temperature must be between"):
        fm.GenerationOptions(temperature=-0.5)


def test_generation_options_repr():
    """Test GenerationOptions __repr__."""
    import fm

    opts = fm.GenerationOptions(temperature=0.7, max_response_tokens=100)
    repr_str = repr(opts)
    assert "GenerationOptions" in repr_str
    assert "temperature=0.7" in repr_str
    assert "max_response_tokens=100" in repr_str


def test_context_limit():
    """Test ContextLimit creation."""
    import fm

    limit = fm.ContextLimit(4096)
    assert limit.max_tokens == 4096
    assert limit.reserved_response_tokens == 0
    assert limit.chars_per_token == 4

    limit = fm.ContextLimit(4096, reserved_response_tokens=512, chars_per_token=3)
    assert limit.max_tokens == 4096
    assert limit.reserved_response_tokens == 512
    assert limit.chars_per_token == 3


def test_context_limit_default_on_device():
    """Test ContextLimit.default_on_device()."""
    import fm

    limit = fm.ContextLimit.default_on_device()
    assert limit.max_tokens == 4096
    assert limit.reserved_response_tokens == 512
    assert limit.chars_per_token == 4


def test_estimate_tokens():
    """Test estimate_tokens function."""
    import fm

    # 4 characters = 1 token at default 4 chars/token
    assert fm.estimate_tokens("abcd", 4) == 1
    assert fm.estimate_tokens("abcd", 3) == 2  # ceil(4/3) = 2

    # Longer text
    text = "Hello, World!"
    tokens = fm.estimate_tokens(text)
    assert tokens > 0


def test_tool_output():
    """Test ToolOutput creation."""
    import fm

    output = fm.ToolOutput("Hello, World!")
    assert output.content == "Hello, World!"


def test_schema_builder():
    """Test Schema builder."""
    import fm

    # String schema
    s = fm.Schema.string()
    d = s.to_dict()
    assert d["type"] == "string"

    # Integer schema with constraints (methods return new instance, use chaining)
    s = fm.Schema.integer().minimum(0).maximum(100)
    d = s.to_dict()
    assert d["type"] == "integer"
    assert d["minimum"] == 0
    assert d["maximum"] == 100

    # Object schema (methods return new instance, use chaining)
    s = (
        fm.Schema.object()
        .property("name", fm.Schema.string(), required=True)
        .property("age", fm.Schema.integer())
    )
    d = s.to_dict()
    assert d["type"] == "object"
    assert "name" in d["properties"]
    assert "age" in d["properties"]
    assert "name" in d["required"]


def test_schema_array():
    """Test Schema.array()."""
    import fm

    items_schema = fm.Schema.string()
    s = fm.Schema.array(items_schema)
    d = s.to_dict()
    assert d["type"] == "array"
    assert d["items"]["type"] == "string"


def test_schema_to_json():
    """Test Schema.to_json()."""
    import fm
    import json

    s = fm.Schema.object()
    s.property("name", fm.Schema.string(), required=True)
    json_str = s.to_json()

    # Should be valid JSON
    parsed = json.loads(json_str)
    assert parsed["type"] == "object"


def test_exceptions_exist():
    """Test that all exception classes exist."""
    import fm

    assert issubclass(fm.FmError, Exception)
    assert issubclass(fm.ModelNotAvailableError, fm.FmError)
    assert issubclass(fm.DeviceNotEligibleError, fm.FmError)
    assert issubclass(fm.AppleIntelligenceNotEnabledError, fm.FmError)
    assert issubclass(fm.ModelNotReadyError, fm.FmError)
    assert issubclass(fm.GenerationError, fm.FmError)
    assert issubclass(fm.ToolCallError, fm.FmError)
    assert issubclass(fm.JsonError, fm.FmError)


# Integration tests - require Apple Intelligence

@pytest.mark.integration
def test_system_language_model():
    """Test SystemLanguageModel creation (requires Apple Intelligence)."""
    import fm

    model = fm.SystemLanguageModel()
    # Check availability (may or may not be available)
    _ = model.is_available
    _ = model.availability


@pytest.mark.integration
def test_session_creation():
    """Test Session creation (requires Apple Intelligence)."""
    import fm

    model = fm.SystemLanguageModel()
    if not model.is_available:
        pytest.skip("Apple Intelligence not available")

    session = fm.Session(model)
    assert not session.is_responding


@pytest.mark.integration
def test_session_with_instructions():
    """Test Session with instructions (requires Apple Intelligence)."""
    import fm

    model = fm.SystemLanguageModel()
    if not model.is_available:
        pytest.skip("Apple Intelligence not available")

    session = fm.Session(model, instructions="You are a helpful assistant.")
    assert not session.is_responding


@pytest.mark.integration
def test_respond():
    """Test basic respond (requires Apple Intelligence)."""
    import fm

    model = fm.SystemLanguageModel()
    if not model.is_available:
        pytest.skip("Apple Intelligence not available")

    session = fm.Session(model)
    response = session.respond("Say hello")
    assert response.content
    assert len(response) > 0


@pytest.mark.integration
def test_stream_response():
    """Test streaming response (requires Apple Intelligence)."""
    import fm

    model = fm.SystemLanguageModel()
    if not model.is_available:
        pytest.skip("Apple Intelligence not available")

    session = fm.Session(model)
    chunks = []

    def on_chunk(chunk):
        chunks.append(chunk)

    session.stream_response("Count to 3", on_chunk)
    assert len(chunks) > 0


@pytest.mark.integration
def test_respond_structured():
    """Test structured response (requires Apple Intelligence)."""
    import fm

    model = fm.SystemLanguageModel()
    if not model.is_available:
        pytest.skip("Apple Intelligence not available")

    session = fm.Session(model)
    schema = {
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "count": {"type": "integer"},
        },
        "required": ["name", "count"],
    }

    result = session.respond_structured("Generate a person with name 'Alice' and count 5", schema)
    assert isinstance(result, dict)
    assert "name" in result or "count" in result


@pytest.mark.integration
def test_tool_calling():
    """Test tool calling (requires Apple Intelligence)."""
    import fm

    model = fm.SystemLanguageModel()
    if not model.is_available:
        pytest.skip("Apple Intelligence not available")

    class EchoTool:
        name = "echo"
        description = "Echoes the input message"
        arguments_schema = {
            "type": "object",
            "properties": {
                "message": {"type": "string"},
            },
            "required": ["message"],
        }

        def call(self, args):
            return f"Echo: {args.get('message', '')}"

    session = fm.Session(model, tools=[EchoTool()])
    response = session.respond("Use the echo tool with message 'hello'")
    # The model should use the tool
    assert response.content
