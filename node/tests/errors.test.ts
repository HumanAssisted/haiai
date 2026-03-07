import { describe, it, expect } from 'vitest';
import {
  HaiError,
  AuthenticationError,
  HaiConnectionError,
  WebSocketError,
  RegistrationError,
  BenchmarkError,
  SSEError,
  HaiApiError,
} from '../src/errors.js';

describe('errors', () => {
  describe('HaiError (base)', () => {
    it('extends Error', () => {
      const err = new HaiError('test');
      expect(err instanceof Error).toBe(true);
      expect(err instanceof HaiError).toBe(true);
    });

    it('sets name to HaiError', () => {
      expect(new HaiError('x').name).toBe('HaiError');
    });

    it('stores statusCode', () => {
      const err = new HaiError('fail', 503);
      expect(err.statusCode).toBe(503);
    });

    it('stores responseData', () => {
      const data = { error: 'details' };
      const err = new HaiError('fail', 500, data);
      expect(err.responseData).toEqual(data);
    });

    it('statusCode defaults to undefined', () => {
      expect(new HaiError('x').statusCode).toBeUndefined();
    });
  });

  describe('AuthenticationError', () => {
    it('extends HaiError', () => {
      const err = new AuthenticationError('bad auth', 401);
      expect(err instanceof HaiError).toBe(true);
      expect(err instanceof Error).toBe(true);
    });

    it('sets name to AuthenticationError', () => {
      expect(new AuthenticationError('x').name).toBe('AuthenticationError');
    });

    it('stores statusCode', () => {
      expect(new AuthenticationError('x', 401).statusCode).toBe(401);
    });
  });

  describe('HaiConnectionError', () => {
    it('extends HaiError', () => {
      const err = new HaiConnectionError('timeout');
      expect(err instanceof HaiError).toBe(true);
    });

    it('sets name', () => {
      expect(new HaiConnectionError('x').name).toBe('HaiConnectionError');
    });

    it('does not set statusCode', () => {
      expect(new HaiConnectionError('x').statusCode).toBeUndefined();
    });
  });

  describe('WebSocketError', () => {
    it('extends HaiError', () => {
      const err = new WebSocketError('ws fail');
      expect(err instanceof HaiError).toBe(true);
    });

    it('sets name', () => {
      expect(new WebSocketError('x').name).toBe('WebSocketError');
    });

    it('stores statusCode', () => {
      expect(new WebSocketError('x', 1008).statusCode).toBe(1008);
    });
  });

  describe('RegistrationError', () => {
    it('extends HaiError', () => {
      const err = new RegistrationError('reg fail');
      expect(err instanceof HaiError).toBe(true);
    });

    it('sets name', () => {
      expect(new RegistrationError('x').name).toBe('RegistrationError');
    });

    it('stores statusCode', () => {
      expect(new RegistrationError('x', 409).statusCode).toBe(409);
    });
  });

  describe('BenchmarkError', () => {
    it('extends HaiError', () => {
      const err = new BenchmarkError('bench fail');
      expect(err instanceof HaiError).toBe(true);
    });

    it('sets name', () => {
      expect(new BenchmarkError('x').name).toBe('BenchmarkError');
    });

    it('stores statusCode', () => {
      expect(new BenchmarkError('x', 422).statusCode).toBe(422);
    });
  });

  describe('SSEError', () => {
    it('extends HaiError', () => {
      const err = new SSEError('sse fail');
      expect(err instanceof HaiError).toBe(true);
    });

    it('sets name', () => {
      expect(new SSEError('x').name).toBe('SSEError');
    });
  });

  describe('HaiApiError', () => {
    it('extends HaiError', () => {
      const err = new HaiApiError('api fail', 500, { detail: 'info' });
      expect(err instanceof HaiError).toBe(true);
    });

    it('sets name', () => {
      expect(new HaiApiError('x').name).toBe('HaiApiError');
    });

    it('stores responseData', () => {
      const data = { field: 'value' };
      const err = new HaiApiError('x', 400, data);
      expect(err.responseData).toEqual(data);
    });
  });

  describe('error hierarchy catch patterns', () => {
    it('catch HaiError catches all sub-errors', () => {
      const errors = [
        new AuthenticationError('a', 401),
        new HaiConnectionError('b'),
        new WebSocketError('c'),
        new RegistrationError('d'),
        new BenchmarkError('e'),
        new SSEError('f'),
        new HaiApiError('g', 500),
      ];
      for (const err of errors) {
        expect(err instanceof HaiError).toBe(true);
      }
    });

    it('specific catch does not match siblings', () => {
      const err = new AuthenticationError('a', 401);
      expect(err instanceof RegistrationError).toBe(false);
      expect(err instanceof BenchmarkError).toBe(false);
    });
  });
});
