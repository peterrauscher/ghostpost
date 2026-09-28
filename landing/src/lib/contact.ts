// Shared by the waitlist form (instant feedback) and the Worker (authoritative check).
export interface Contact {
  kind: 'email' | 'phone';
  /** Normalized: lowercased email, or E.164 phone number. */
  value: string;
}

const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]{2,}$/;
const PHONE_CHARS = /^\+?[\d\s().-]+$/;

/**
 * Accepts an email address or a phone number. Numbers without a leading `+` are read as US/Canada
 * (10 digits, or 11 starting with 1); anything international must include its `+` country code.
 */
export function parseContact(raw: string): Contact | null {
  const input = raw.trim();
  if (input.length === 0 || input.length > 254) return null;

  if (input.includes('@')) {
    const email = input.toLowerCase();
    return EMAIL.test(email) ? { kind: 'email', value: email } : null;
  }

  if (!PHONE_CHARS.test(input)) return null;
  const digits = input.replace(/\D/g, '');
  if (input.startsWith('+')) {
    return digits.length >= 8 && digits.length <= 15 ? { kind: 'phone', value: `+${digits}` } : null;
  }
  if (digits.length === 10) return { kind: 'phone', value: `+1${digits}` };
  if (digits.length === 11 && digits.startsWith('1')) return { kind: 'phone', value: `+${digits}` };
  return null;
}
