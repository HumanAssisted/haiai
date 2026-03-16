"""Tests for haiai.errors module."""

from __future__ import annotations

import pytest

from haiai.errors import (
    AuthenticationError,
    BenchmarkError,
    HaiApiError,
    HaiAuthError,
    HaiConnectionError,
    HaiError,
    RegistrationError,
    SSEError,
    WebSocketError,
)


class TestHaiError:
    def test_basic_message(self) -> None:
        err = HaiError("something broke")
        assert str(err) == "something broke"
        assert err.message == "something broke"
        assert err.status_code is None
        assert err.response_data == {}

    def test_with_status_code(self) -> None:
        err = HaiError("forbidden", status_code=403)
        assert str(err) == "forbidden (HTTP 403)"
        assert err.status_code == 403

    def test_with_response_data(self) -> None:
        err = HaiError("err", response_data={"detail": "x"})
        assert err.response_data == {"detail": "x"}

    def test_is_exception(self) -> None:
        with pytest.raises(HaiError):
            raise HaiError("oops")

    def test_from_response_with_json(self) -> None:
        class FakeResp:
            status_code = 500
            def json(self):
                return {"error": "internal"}

        err = HaiError.from_response(FakeResp())
        assert err.status_code == 500
        assert "internal" in str(err)

    def test_from_response_no_json(self) -> None:
        class FakeResp:
            status_code = 502
            def json(self):
                raise ValueError("no json")

        err = HaiError.from_response(FakeResp(), "fallback msg")
        assert "fallback msg" in str(err)


class TestHaiApiError:
    def test_inherits_hai_error(self) -> None:
        assert issubclass(HaiApiError, HaiError)

    def test_body_attribute(self) -> None:
        err = HaiApiError("bad", status_code=400, body='{"msg":"bad"}')
        assert err.body == '{"msg":"bad"}'
        assert err.status_code == 400


class TestHaiAuthError:
    def test_inherits_api_error(self) -> None:
        assert issubclass(HaiAuthError, HaiApiError)

    def test_catchable_as_hai_error(self) -> None:
        with pytest.raises(HaiError):
            raise HaiAuthError("auth fail", status_code=401)


class TestHaiConnectionError:
    def test_inherits_hai_error(self) -> None:
        assert issubclass(HaiConnectionError, HaiError)


class TestRegistrationError:
    def test_inherits_hai_error(self) -> None:
        assert issubclass(RegistrationError, HaiError)

    def test_with_status_code(self) -> None:
        err = RegistrationError("already exists", status_code=409)
        assert err.status_code == 409


class TestBenchmarkError:
    def test_inherits_hai_error(self) -> None:
        assert issubclass(BenchmarkError, HaiError)


class TestSSEError:
    def test_inherits_hai_error(self) -> None:
        assert issubclass(SSEError, HaiError)


class TestWebSocketError:
    def test_inherits_hai_error(self) -> None:
        assert issubclass(WebSocketError, HaiError)


class TestAliases:
    def test_authentication_error_is_hai_auth_error(self) -> None:
        assert AuthenticationError is HaiAuthError

    def test_authentication_error_catchable(self) -> None:
        with pytest.raises(HaiAuthError):
            raise AuthenticationError("fail", status_code=401)
