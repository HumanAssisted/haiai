/** Base error class for all HAI SDK errors. */
export class HaiError extends Error {
  statusCode?: number;
  responseData?: Record<string, unknown>;
  /** Structured error code (e.g. 'JACS_NOT_LOADED'). */
  errorCode: string;
  /** Developer-facing hint describing how to fix the issue. */
  action: string;

  constructor(message: string, statusCode?: number, responseData?: Record<string, unknown>,
              errorCode: string = '', action: string = '') {
    super(action ? `${message}. ${action}` : message);
    this.name = 'HaiError';
    this.statusCode = statusCode;
    this.responseData = responseData;
    this.errorCode = errorCode;
    this.action = action;
  }
}

/** Thrown when JACS authentication is rejected. */
export class AuthenticationError extends HaiError {
  constructor(message: string, statusCode?: number) {
    super(message, statusCode);
    this.name = 'AuthenticationError';
  }
}

/** Thrown when connection to HAI fails. */
export class HaiConnectionError extends HaiError {
  constructor(message: string) {
    super(message);
    this.name = 'HaiConnectionError';
  }
}

/** Thrown for WebSocket-specific errors. */
export class WebSocketError extends HaiError {
  constructor(message: string, statusCode?: number) {
    super(message, statusCode);
    this.name = 'WebSocketError';
  }
}

/** Thrown when agent registration fails. */
export class RegistrationError extends HaiError {
  constructor(message: string, statusCode?: number) {
    super(message, statusCode);
    this.name = 'RegistrationError';
  }
}

/** Thrown when a benchmark operation fails. */
export class BenchmarkError extends HaiError {
  constructor(message: string, statusCode?: number) {
    super(message, statusCode);
    this.name = 'BenchmarkError';
  }
}

/** Thrown when SSE streaming fails. */
export class SSEError extends HaiError {
  constructor(message: string, statusCode?: number) {
    super(message, statusCode);
    this.name = 'SSEError';
  }
}

/** General API error (non-auth, non-connection). */
export class HaiApiError extends HaiError {
  /** Raw response body text. */
  body: string;

  constructor(message: string, statusCode?: number, responseData?: Record<string, unknown>, errorCode: string = '', body: string = '') {
    super(message, statusCode, responseData, errorCode);
    this.name = 'HaiApiError';
    this.body = body;
  }
}

/** Thrown when the agent's email is not yet active (status is "allocated"). */
export class EmailNotActiveError extends HaiApiError {
  constructor(message: string, statusCode: number = 403, body: string = '') {
    super(message, statusCode, undefined, 'EMAIL_NOT_ACTIVE', body);
    this.name = 'EmailNotActiveError';
  }
}

/** Thrown when the recipient address cannot be resolved. */
export class RecipientNotFoundError extends HaiApiError {
  constructor(message: string, statusCode: number = 400, body: string = '') {
    super(message, statusCode, undefined, 'RECIPIENT_NOT_FOUND', body);
    this.name = 'RecipientNotFoundError';
  }
}

/** Thrown when the agent has exceeded its email rate limit. */
export class RateLimitedError extends HaiApiError {
  constructor(message: string, statusCode: number = 429, body: string = '') {
    super(message, statusCode, undefined, 'RATE_LIMITED', body);
    this.name = 'RateLimitedError';
  }
}
