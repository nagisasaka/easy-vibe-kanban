import { describe, expect, it } from 'vitest';
import { previewProxyUrl } from './previewProxyUrl';

describe('previewProxyUrl', () => {
  const source = new URL('http://localhost:5173/path?q=1#section');
  it('preserves local and relay previews', () => {
    expect(previewProxyUrl(source, '5173', 3009)?.href).toBe(
      'http://5173.localhost:3009/path?q=1#section'
    );
    expect(
      previewProxyUrl(source, '5173', 3009, null, 'host-id')?.hostname
    ).toBe('5173--host-id.localhost');
  });
  it('uses HTTPS with no internal port exposed', () => {
    expect(
      previewProxyUrl(source, '5173', 3001, 'preview.example.com')?.href
    ).toBe('https://5173.preview.example.com/path?q=1#section');
  });
  it('does not interpret a double-slash path as a host', () => {
    expect(
      previewProxyUrl(
        new URL('http://localhost:5173//evil.test/a'),
        '5173',
        3001,
        'preview.example.com'
      )?.hostname
    ).toBe('5173.preview.example.com');
  });
  it.each(['0', '80', '443', '3000', '3001', '8443', '65536', '1.evil'])(
    'rejects invalid or reserved public port %s',
    (port) => {
      expect(
        previewProxyUrl(source, port, 3001, 'preview.example.com')
      ).toBeNull();
    }
  );
  it('rejects malformed domains and unsupported relay targets on the single-host distribution', () => {
    expect(previewProxyUrl(source, '5173', 3001, 'evil.test/path')).toBeNull();
    expect(
      previewProxyUrl(source, '5173', 3001, 'preview.example.com', 'host-id')
    ).toBeNull();
  });
});
