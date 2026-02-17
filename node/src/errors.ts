/** Base error class for all HAI SDK errors. */
export class HaiError extends Error {
  statusCode?: number;
  responseData?: Record<string, unknown>;

  constructor(message: string, statusCode?: number, responseData?: Record<string, unknown>) {
    super(message);
    this.name = 'HaiError';
    this.statusCode = statusCode;
    this.responseData = responseData;
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
  constructor(message: string, statusCode?: number, responseData?: Record<string, unknown>) {
    super(message, statusCode, responseData);
    this.name = 'HaiApiError';
  }
}
